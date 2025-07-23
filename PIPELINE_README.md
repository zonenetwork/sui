# Bitbucket Pipeline Configuration

This repository contains an efficient Bitbucket pipeline configuration that dynamically builds and pushes Docker images for all services in the `docker/` directory to Azure Container Registry.

## Overview

The pipeline **automatically discovers** all services in the docker directory by folder name, using their respective `build.sh` scripts where available, and direct Docker builds for services with only Dockerfiles. **No manual service configuration required!**

## Environment Variables Setup

Before using this pipeline, you need to configure the following repository variables in your Bitbucket repository settings:

1. **REGISTRY_URL**: `zoneswitch.azurecr.io`
2. **REGISTRY_USERNAME**: `zoneswitch`
3. **REGISTRY_PASSWORD**: `tester`

### Setting Environment Variables in Bitbucket:

1. Go to your repository in Bitbucket
2. Navigate to **Repository settings** → **Pipelines** → **Repository variables**
3. Add the three variables listed above
4. Make sure to mark `REGISTRY_PASSWORD` as "Secured" for security

## Versioning Strategy

### Master Branch
- **Format**: `x.x.x` (major.minor.patch)
- **Logic**: Uses the build number to create semantic versioning
  - First digit = Major version
  - Second digit = Minor version  
  - Remaining digits = Patch version
- **Example**: Build #123 → Version `1.2.3`
- **Additional**: Also tagged as `latest`

### Other Branches
- **Format**: `{BUILD_NUMBER}-dev`
- **Example**: Build #456 on feature branch → Version `456-dev`

## Services Built

**Dynamic Service Discovery**: The pipeline automatically discovers and builds ALL services in the `docker/` directory. Service names are determined by the **folder name** containing the Dockerfile.

### Auto-Detection Logic:
- **With build.sh**: Uses the build script if present
- **Dockerfile only**: Falls back to direct docker build
- **Service name**: Uses the folder name as the image name
- **No manual configuration**: Add new services by just creating a new folder

## Pipeline Triggers

### Automatic Builds
The pipeline uses **changeset conditions** to efficiently build services:

- Builds trigger when files in `docker/**`, `crates/**`, `Cargo.toml`, `Cargo.lock` change
- **Dynamic discovery**: No need to manually add new services to the pipeline
- **Efficient execution**: Only runs when relevant files change

### Manual Builds
- **`build-all`**: Build all discovered services regardless of changes
- **`build-service`**: Build a specific service by providing the folder name

## Pipeline Features

### Dynamic Service Discovery
- **Zero configuration**: Automatically finds all services in `docker/` folders
- **Auto-detection**: Uses `build.sh` if present, falls back to direct Docker build
- **Scalable**: Add new services by just creating a folder with a Dockerfile

### Efficient Execution
- **Smart triggering**: Only runs when relevant files change
- **Single job**: All services built in one efficient step
- **Resource optimization**: Sequential builds with proper cleanup

### Build Optimization
- **Docker BuildKit**: Faster builds with improved caching
- **Semantic versioning**: Automatic version generation from build numbers
- **Registry integration**: Seamless Azure Container Registry pushing

## Build Arguments

Each Docker image is built with the following build arguments:
- `BUILD_DATE`: Current UTC date
- `GIT_REVISION`: Short Git commit hash
- `PROFILE`: Set to "release" by default (or "bench" for debug builds)

## Image Naming Convention

Images are pushed to the registry with the following naming pattern:
```
{REGISTRY_URL}/{SERVICE_NAME}:{VERSION_TAG}
```

**Examples:**
- Master branch: `zoneswitch.azurecr.io/sui-node:1.2.3`
- Feature branch: `zoneswitch.azurecr.io/sui-node:456-dev`
- Latest (master only): `zoneswitch.azurecr.io/sui-node:latest`

## Usage

### Automatic Usage
1. Commit changes to any service in the `docker/` directory
2. Push to any branch
3. Pipeline automatically detects changes and builds affected services

### Manual Trigger
1. Go to Pipelines in Bitbucket
2. Click "Run pipeline"
3. Select "Custom: manual-build-all"
4. This will build all services regardless of changes

## Troubleshooting

### Build Failures
- Check the build logs for specific error messages
- Ensure all required dependencies are available
- Verify Rust compilation succeeds locally

### Registry Authentication Issues
- Verify the registry credentials are correctly set
- Check that the registry URL is accessible from Bitbucket
- Ensure the service account has push permissions

### Version Format Issues
- The versioning logic assumes build numbers are numeric
- For very large build numbers, adjust the version parsing logic as needed

## Pipeline Structure

The pipeline configuration uses:
- **Definitions**: Reusable step templates
- **Parallel execution**: Multiple services build simultaneously  
- **Conditions**: Smart triggering based on file changes
- **Services**: Docker-in-Docker for container builds
- **Variables**: Secure credential management

## Customization

To add a new service:
1. Add the service directory under `docker/`
2. Create a new step in the pipeline for the service
3. Add appropriate changeset conditions
4. Follow the naming convention for consistency

To modify versioning:
1. Update the version calculation logic in each step
2. Ensure consistency across all service builds
3. Test with different build numbers

## Security Considerations

- Registry credentials are stored as secured repository variables
- Build contexts are isolated per service
- Images are scanned and tagged with build metadata
- No sensitive information is exposed in build logs 