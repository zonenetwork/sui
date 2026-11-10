// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

module zone_gasless::workload;

use sui::coin::{Self, TreasuryCap};
use sui::event;
use sui::vec_set::{Self, VecSet};

public struct WORKLOAD has drop {}
public struct Admin has key, store { id: UID }
public struct Counter has key { id: UID, value: u64, references: VecSet<u64> }
public struct Ticket has key, store { id: UID, value: u64 }
public struct Payment has copy, drop { reference: u64, amount: u64 }

fun init(witness: WORKLOAD, ctx: &mut TxContext) {
    let (treasury, metadata) = coin::create_currency(
        witness, 9, b"ZONE_TEST", b"Zone test asset", b"Gasless qualification fixture",
        option::none(), ctx,
    );
    transfer::public_freeze_object(metadata);
    transfer::public_transfer(treasury, ctx.sender());
    transfer::public_transfer(Admin { id: object::new(ctx) }, ctx.sender());
    transfer::share_object(Counter { id: object::new(ctx), value: 0, references: vec_set::empty() });
}

public fun pay(counter: &mut Counter, _: &Admin, reference: u64, amount: u64, iterations: u64) {
    assert!(!counter.references.contains(&reference), 1);
    let mut i = 0;
    while (i < iterations) { i = i + 1 };
    counter.references.insert(reference);
    counter.value = counter.value + amount;
    event::emit(Payment { reference, amount });
}

public fun shared_only(counter: &mut Counter, amount: u64) {
    counter.value = counter.value + amount;
}

public fun payload(_bytes: vector<u8>) {}

public fun abort_after_write(counter: &mut Counter) {
    counter.value = 999999;
    abort 7
}

public fun create(value: u64, ctx: &mut TxContext) {
    transfer::public_transfer(Ticket { id: object::new(ctx), value }, ctx.sender());
}

public fun mutate(ticket: &mut Ticket, delta: u64) { ticket.value = ticket.value + delta }
public fun destroy(ticket: Ticket) { let Ticket { id, value: _ } = ticket; id.delete() }

public fun mint(cap: &mut TreasuryCap<WORKLOAD>, amount: u64, recipient: address, ctx: &mut TxContext) {
    transfer::public_transfer(coin::mint(cap, amount, ctx), recipient);
}
