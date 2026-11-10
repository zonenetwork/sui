#!/usr/bin/env python3
# Copyright (c) Mysten Labs, Inc.
# SPDX-License-Identifier: Apache-2.0
"""Qualify a Zone build on a NEW task-owned four-validator network (Linux).

Usage: python3 scripts/zone-gasless-smoke.py --sui target/debug/sui \
    --work-dir /tmp/zone-gasless-unique --rpc-port 18945
The directory must not exist. No faucet, protocol override or external network is used.
Keystores remain inside the private work directory; the report contains no keys.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import time


def decode_json(output):
    decoder = json.JSONDecoder()
    for match in re.finditer(r"(?m)^\s*(?=[\[\{\"])", output):
        try:
            value, end = decoder.raw_decode(output[match.end():])
            if not output[match.end() + end:].strip():
                return value
        except json.JSONDecodeError:
            pass
    raise ValueError("CLI returned no complete JSON value: " + output[-2000:])


class Network:
    def __init__(self, sui, root, port):
        self.sui, self.root, self.port = str(sui), root, port
        self.config = root / "network"
        self.proc = None
        self.log = None
        self.records = []

    def command(self, *args, json_output=True, timeout=240, allow_error=False):
        command = [self.sui, "client", "--client.config", str(self.config / "client.yaml")]
        if json_output:
            command += ["--json"]
        result = subprocess.run(command + list(args), capture_output=True, text=True, timeout=timeout)
        if result.returncode and not allow_error:
            raise RuntimeError(f"CLI {args[0]} failed: {result.stderr[-3000:]} {result.stdout[-3000:]}")
        if allow_error:
            return result
        return decode_json(result.stdout) if json_output else result.stdout

    def start(self):
        with socket.socket() as available:
            available.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            available.bind(("127.0.0.1", self.port))
        self.log = (self.root / f"network-{len(self.records)}.log").open("w")
        self.proc = subprocess.Popen(
            [self.sui, "start", "--network.config", str(self.config),
             "--fullnode-rpc-port", str(self.port)],
            stdout=self.log, stderr=subprocess.STDOUT, start_new_session=True,
        )
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError("Network exited during startup; inspect its log")
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=1):
                    return
            except OSError:
                time.sleep(0.5)
        raise TimeoutError("Fullnode did not start")

    def stop(self):
        if self.proc is not None and self.proc.poll() is None:
            os.killpg(self.proc.pid, signal.SIGTERM)
            try:
                self.proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                os.killpg(self.proc.pid, signal.SIGKILL)
                self.proc.wait()
        if self.log:
            self.log.close()

    def wait_for_object(self, object_id, predicate=lambda _: True):
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError("Network stopped while recovering")
            try:
                object_ = self.command("object", object_id, timeout=10)
                if predicate(object_):
                    return object_
            except (RuntimeError, ValueError, subprocess.TimeoutExpired):
                pass
            time.sleep(1)
        raise TimeoutError("Fullnode did not recover the expected object")

    def connect(self, alias):
        rpc = f"http://127.0.0.1:{self.port}"
        deadline = time.monotonic() + 90
        last_error = ""
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError("Network exited before RPC readiness")
            try:
                environments = self.command("envs", timeout=10)[0]
                existing = next((env for env in environments if env["alias"] == alias), None)
                if existing:
                    assert existing["rpc"] == rpc, "Saved environment points to another network"
                    self.command("switch", "--env", alias, timeout=10)
                    chain = self.command("chain-identifier", timeout=10)
                    self.command("object", "0x6", timeout=10)
                    return chain
                added = self.command("new-env", "--alias", alias, "--rpc", rpc,
                                     allow_error=True, timeout=10)
                last_error = added.stderr + added.stdout
            except (RuntimeError, ValueError, subprocess.TimeoutExpired) as error:
                last_error = str(error)
            time.sleep(1)
        raise TimeoutError("Fullnode RPC did not become ready: " + last_error[-1500:])

    def check_tx(self, label, response, success=True):
        effects = response["effects"]
        assert (effects["status"]["status"] == "success") == success, response
        gas = effects["gasUsed"]
        assert all(int(value) == 0 for value in gas.values()), gas
        self.records.append({"check": label, "digest": response.get("digest"),
                             "epoch": effects["executedEpoch"], "gas": gas,
                             "status": effects["status"]})
        print(f"PASS {label}", flush=True)
        return response


def zone_contracts(network, source, repo, sender, recipient, chain, report):
    """Use exact local Move sources; adapt only copied manifests for the new genesis."""
    framework = repo / "crates/sui-framework/packages"
    destination = network.root / "zone-contracts"
    publications = {}
    hashes = {}
    for name, dependencies in [("common", []), ("coins", ["common"]),
                               ("registry", ["common"]),
                               ("fundingandliquidation", ["common", "coins"])]:
        package = destination / name
        original = source / "src" / name
        shutil.copytree(original / "sources", package / "sources")
        for file in sorted(original.rglob("*.move")):
            if "sources" in file.parts:
                hashes[str(file.relative_to(source))] = hashlib.sha256(file.read_bytes()).hexdigest()
        manifest = (f'[package]\nname = "{name}"\nedition = "2024"\nimplicit-dependencies = false\n'
                    f'\n[dependencies]\nsui = {{ local = "{framework / "sui-framework"}" }}\n'
                    f'std = {{ local = "{framework / "move-stdlib"}" }}\n')
        for dependency in dependencies:
            manifest += f'{dependency} = {{ local = "../{dependency}" }}\n'
        manifest += f'\n[environments]\nzone-smoke = "{chain}"\n'
        (package / "Move.toml").write_text(manifest)
        publications[name] = network.check_tx("zone_publish_" + name, network.command("publish", str(package)))
    report["zone_contract_source_sha256"] = hashes
    def package_id(name):
        return next(o["packageId"] for o in publications[name]["objectChanges"] if o["type"] == "published")
    def created(response, suffix):
        return next(o["objectId"] for o in response["objectChanges"]
                    if suffix in o.get("objectType", "") and o["type"] == "created")
    def call(module, function, *values, extra=()):
        return network.command("call", "--package", package_id("fundingandliquidation"),
                               "--module", module, "--function", function,
                               "--args", *map(str, values), *extra)
    treasury = created(publications["coins"], "::coin::TreasuryCap<")
    funding_admin = created(publications["fundingandliquidation"], "::funding_caps::FundingAdminCap")
    liquidation_admin = created(publications["fundingandliquidation"], "::liquidation_caps::LiquidationAdminCap")
    issued = network.check_tx("zone_funding_capability", call("funding", "issue_funding_capability", funding_admin, sender))
    funding_cap = created(issued, "::funding_caps::FundingCap")
    shard_result = network.check_tx("zone_funding_shard", call("funding", "create_funding_shard_for_params", sender, treasury))
    funding_shard = created(shard_result, "::funding_data::FundingShard")
    funded = network.check_tx("zone_fund_institution", call("funding", "fundInstitution",
        funding_cap, treasury, funding_shard, "zone-gasless-test-1", sender, 1000, "qualification"))
    assert funded["events"]
    coin = created(funded, "::coin::Coin<")
    assert int(network.command("object", coin)["content"]["balance"]) == 1000
    duplicate = call("funding", "fundInstitution", funding_cap, treasury, funding_shard,
                     "zone-gasless-test-1", sender, 1000, "qualification", extra=("--dry-run",))
    network.check_tx("zone_duplicate_funding_abort", duplicate, success=False)
    issued = network.check_tx("zone_liquidation_capability", call("liquidation",
        "issue_liquidation_capability_with_shard", liquidation_admin, sender, "test-institution", sender, treasury))
    liquidation_cap = created(issued, "::liquidation_caps::LiquidationCap")
    liquidation_shard = created(issued, "::liquidation_data::LiquidationShard")
    liquidated = network.check_tx("zone_liquidate_institution", call("liquidation", "liquidateInstitution",
        liquidation_cap, treasury, liquidation_shard, coin, "zone-gasless-liquidation-1", sender, 500, "qualification"))
    assert liquidated["events"]
    assert int(network.command("object", coin)["content"]["balance"]) == 500
    network.check_tx("zone_transfer_remaining_balance", network.command("transfer", "--to", recipient, "--object-id", coin))
    received = network.command("object", coin)
    assert received["owner"]["AddressOwner"] == recipient
    assert int(received["content"]["balance"]) == 500
    report["zone_packages"] = {name: package_id(name) for name in publications}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sui", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--rpc-port", type=int, default=18945)
    parser.add_argument("--zone-contracts", type=Path, help="Optional local zone.smartcontracts.sui reference")
    args = parser.parse_args()
    root, sui = args.work_dir.resolve(), args.sui.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    repo = Path(__file__).resolve().parents[1]
    network = Network(sui, root, args.rpc_port)
    report = {"binary_sha256": hashlib.sha256(sui.read_bytes()).hexdigest(),
              "binary_version": subprocess.check_output([str(sui), "--version"], text=True).strip(),
              "checks": network.records, "passed": False}
    try:
        network.config.mkdir()
        with (root / "genesis.log").open("w") as log:
            subprocess.run([str(sui), "genesis", "--working-dir", str(network.config),
                            "--committee-size", "4", "--epoch-duration-ms", "30000"],
                           stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
        client_config = network.config / "client.yaml"
        client_config.write_text(client_config.read_text().replace(":9000", f":{args.rpc_port}"))
        network.start()
        report["chain"] = network.connect("zone-smoke")
        genesis_sender = network.command("active-address")
        # Deliberately discard recovery phrases; neither account receives genesis SUI.
        sender = network.command("new-address", "ed25519", "zone-smoke-sender")["address"]
        recipient = network.command("new-address", "ed25519", "zone-smoke-recipient")["address"]
        report["sender"], report["recipient"] = sender, recipient
        network.command("switch", "--address", sender)
        gas_before = network.command("gas", sender)
        assert gas_before["gasCoins"] == [] and int(gas_before["addressMistBalance"]) == 0
        report["gas_before"] = gas_before
        package = root / "workload"
        shutil.copytree(repo / "crates/sui-e2e-tests/tests/data/zone_gasless", package,
                        ignore=shutil.ignore_patterns("build", "Move.lock", "Published.toml"))
        manifest = package / "Move.toml"
        chain = report["chain"]["hex"]
        framework = repo / "crates/sui-framework/packages"
        # CLI publication uses the modern manifest; the Rust test builder uses the legacy one.
        manifest.write_text(
            '[package]\nname = "zone_gasless"\nedition = "2024"\nimplicit-dependencies = false\n'
            f'\n[dependencies]\nsui = {{ local = "{framework / "sui-framework"}" }}\n'
            f'std = {{ local = "{framework / "move-stdlib"}" }}\n'
            f'\n[environments]\nzone-smoke = "{chain}"\n'
        )
        published = network.check_tx("publish_zero_sui", network.command("publish", str(package)))
        changes = published["objectChanges"]
        package_id = next(o["packageId"] for o in changes if o["type"] == "published")
        def object_id(suffix):
            return next(o["objectId"] for o in changes if o.get("objectType", "").endswith(suffix))
        counter, admin = object_id("::workload::Counter"), object_id("::workload::Admin")
        upgrade_cap = object_id("::package::UpgradeCap")
        def call(function, *values, extra=()):
            return network.command("call", "--package", package_id, "--module", "workload",
                                   "--function", function, "--args", *map(str, values), *extra)
        def counter_value():
            object_ = network.command("object", counter)
            assert int(object_["storageRebate"]) == 0
            return int(object_["content"]["value"])
        dry = call("pay", counter, admin, 42, 500, 1000000, extra=("--dry-run",))
        network.check_tx("dry_run_unmetered", dry)
        assert counter_value() == 0
        paid = network.check_tx("move_payment_unmetered", call("pay", counter, admin, 42, 500, 1000000))
        assert counter_value() == 500
        assert len(paid["events"]) == 1
        # A failed simulation must be atomic and charge zero, just as execution does.
        duplicate = call("pay", counter, admin, 42, 500, 0, extra=("--dry-run",))
        network.check_tx("duplicate_reference_abort", duplicate, success=False)
        assert counter_value() == 500
        signed = network.command("call", "--package", package_id, "--module", "workload",
                                 "--function", "shared_only", "--args", counter, "1",
                                 "--serialize-signed-transaction", json_output=False)
        encoded = next(line.strip() for line in signed.splitlines()
                       if re.fullmatch(r"[A-Za-z0-9+/]{80,}={0,2}", line.strip()))
        first = network.check_tx("serialized_signed_shared_call", network.command(
            "execute-combined-signed-tx", "--signed-tx-bytes", encoded))
        retry = network.check_tx("exact_signed_retry", network.command(
            "execute-combined-signed-tx", "--signed-tx-bytes", encoded))
        assert first["digest"] == retry["digest"]
        assert first["effects"] == retry["effects"]
        assert counter_value() == 501
        made = network.check_tx("create_owned", call("create", 7))
        ticket = next(o["objectId"] for o in made["objectChanges"]
                      if o.get("objectType", "").endswith("::workload::Ticket"))
        network.check_tx("mutate_owned", call("mutate", ticket, 3))
        network.check_tx("transfer_owned", network.command("transfer", "--to", recipient, "--object-id", ticket))
        network.command("switch", "--address", recipient)
        network.check_tx("delete_owned", call("destroy", ticket))
        network.command("switch", "--address", sender)
        network.check_tx("upgrade_zero_sui", network.command("upgrade", str(package), "--upgrade-capability", upgrade_cap))
        initial_epoch = int(first["effects"]["executedEpoch"])
        deadline = time.monotonic() + 150
        while time.monotonic() < deadline:
            observed = network.check_tx("epoch_probe", call("shared_only", counter, 0))
            if int(observed["effects"]["executedEpoch"]) >= initial_epoch + 2:
                break
            time.sleep(3)
        else:
            raise TimeoutError("Network did not cross two epochs")
        network.stop()
        network.start()
        network.wait_for_object(counter, lambda o: int(o["content"]["value"]) == 501)
        assert counter_value() == 501
        network.check_tx("persisted_restart", call("shared_only", counter, 1))
        assert counter_value() == 502
        network.command("switch", "--address", genesis_sender)
        explicit_before = network.command("gas", genesis_sender)
        explicit_coin = explicit_before["gasCoins"][0]["gasCoinId"]
        network.check_tx("explicit_coin_zero_price", call("shared_only", counter, 0,
            extra=("--gas", explicit_coin, "--gas-price", "0")))
        assert network.command("gas", genesis_sender) == explicit_before
        network.command("switch", "--address", sender)
        if args.zone_contracts:
            zone_contracts(network, args.zone_contracts.resolve(), repo, sender, recipient, chain, report)
        report["gas_after"] = network.command("gas", sender)
        assert report["gas_before"] == report["gas_after"]
        report["passed"] = True
    finally:
        network.stop()
        (root / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"PASS fresh network; report: {root / 'report.json'}", flush=True)


if __name__ == "__main__":
    main()
