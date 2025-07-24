#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Builds Docker images for selected services and pushes to Azure Container Registry
    
.DESCRIPTION
    This script builds Docker images for selected services from the docker directory.
    It automatically detects build.sh scripts vs direct Dockerfile builds, handles
    semantic versioning, and provides comprehensive logging and cleanup.
    
.PARAMETER Services
    Array of service names to build. If not specified, shows available services.
    
.PARAMETER RegistryUrl
    Azure Container Registry URL as String (required when pushing to registry, optional with -ShowServices or -SkipPush)
    
.PARAMETER RegistryUsername
    Registry username as String (required when pushing to registry, optional with -ShowServices or -SkipPush)
    
.PARAMETER RegistryPassword
    Registry password as String (required when pushing to registry, optional with -ShowServices or -SkipPush)
    
.PARAMETER Version
    Semantic version in x.x.x format (default: auto-generated)
    
.PARAMETER Force
    Force rebuild even if image exists
    
.PARAMETER SkipPush
    Build images but don't push to registry
    
.PARAMETER SkipCleanup
    Don't remove built images after push
    
.PARAMETER ShowServices
    Show available services and exit (no building)
    
.EXAMPLE
    .\build-docker-services.ps1 -Services @("sui-node", "sui-tools")
    
.EXAMPLE
    .\build-docker-services.ps1 -Services @("sui-tools") -SkipPush -Verbose

.EXAMPLE
    .\build-docker-services.ps1 -Version "1.2.3"
    
.EXAMPLE
    .\build-docker-services.ps1 -Services @("sui-node") -Version "2.1.0"
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $false)]
    [string[]]$Services = @(),
    
    [Parameter(Mandatory = $false)]
    [string]$RegistryUrl,
    
    [Parameter(Mandatory = $false)]
    [string]$RegistryUsername,
    
    [Parameter(Mandatory = $false)]
    [string]$RegistryPassword,
    
    [Parameter(Mandatory = $false)]
    [string]$Version = "",
    
    [Parameter(Mandatory = $false)]
    [switch]$Force,
    
    [Parameter(Mandatory = $false)]
    [switch]$SkipPush,
    
    [Parameter(Mandatory = $false)]
    [switch]$SkipCleanup,
    
    [Parameter(Mandatory = $false)]
    [switch]$ShowServices
)

# Global variables
$script:BuiltImages = @()
$script:FailedBuilds = @()
$script:StartTime = Get-Date
$script:DockerBuildKit = "1"
$script:MaxJobs = [Environment]::ProcessorCount

# Available target services
$TargetServices = @(
    "sui-node",
#    "sui-proxy", 
    "sui-tools",
    "sui-services",
#    "sui-kv-rpc",
#    "sui-source-service",
    "sui-indexer"
)

function Write-Banner {
    param([string]$Message)
    
    $border = "=" * 80
    Write-Host $border -ForegroundColor Cyan
    Write-Host " $Message" -ForegroundColor Cyan
    Write-Host $border -ForegroundColor Cyan
}

function Write-Section {
    param([string]$Message)
    
    $border = "-" * 60
    Write-Host ""
    Write-Host $border -ForegroundColor Yellow
    Write-Host " $Message" -ForegroundColor Yellow
    Write-Host $border -ForegroundColor Yellow
}

function Write-Success {
    param([string]$Message)
    Write-Host "✅ $Message" -ForegroundColor Green
}

function Write-Error-Custom {
    param([string]$Message)
    Write-Host "❌ $Message" -ForegroundColor Red
}

function Write-Warning-Custom {
    param([string]$Message)
    Write-Host "⚠️  $Message" -ForegroundColor Yellow
}

function Write-Info {
    param([string]$Message)
    Write-Host "ℹ️  $Message" -ForegroundColor Blue
}

function Get-GitBranch {
    try {
        $branch = git rev-parse --abbrev-ref HEAD 2>$null
        if ($LASTEXITCODE -eq 0) {
            Write-Verbose "Current git branch: $branch"
            return $branch.Trim()
        }
    } catch {
        Write-Warning-Custom "Could not determine git branch: $($_.Exception.Message)"
    }
    
    Write-Warning-Custom "Defaulting to 'main' branch"
    return "main"
}

function Get-GitRevision {
    try {
        $revision = git rev-parse --short HEAD 2>$null
        if ($LASTEXITCODE -eq 0) {
            Write-Verbose "Git revision: $revision"
            return $revision.Trim()
        }
    } catch {
        Write-Warning-Custom "Could not get git revision: $($_.Exception.Message)"
    }
    
    $fallback = Get-Date -Format "yyyyMMdd"
    Write-Warning-Custom "Using date-based revision: $fallback"
    return $fallback
}

function Get-SemanticVersion {
    param([string]$Version, [string]$Branch)
    
    if ([string]::IsNullOrEmpty($Version)) {
        # Auto-generate version if not provided
        $timestamp = Get-Date -Format "HHmmss"
        Write-Verbose "Auto-generating version from timestamp: $timestamp"
        
        if ($Branch -eq "master" -or $Branch -eq "main") {
            # Format as x.x.x (major.minor.patch) from timestamp
            $Major = $timestamp.Substring(0, 1)
            $Minor = if ($timestamp.Length -ge 3) { $timestamp.Substring(1, 2).TrimStart('0') } else { "0" }
            $Patch = if ($timestamp.Length -ge 4) { $timestamp.Substring(3).TrimStart('0') } else { "0" }
            
            if ([string]::IsNullOrEmpty($Minor)) { $Minor = "0" }
            if ([string]::IsNullOrEmpty($Patch)) { $Patch = "0" }
            
            $version = "$Major.$Minor.$Patch"
        } else {
            $version = "$timestamp-dev"
        }
    } else {
        # Use provided semantic version
        if ($Branch -eq "master" -or $Branch -eq "main") {
            # Validate semantic version format
            if ($Version -match '^\d+\.\d+\.\d+$') {
                $version = $Version
            } else {
                Write-Warning-Custom "Invalid semantic version format '$Version'. Expected x.x.x format."
                Write-Info "Using provided version as-is: $Version"
                $version = $Version
            }
        } else {
            # For non-master branches, append -dev if not already present
            if ($Version -match '-dev$') {
                $version = $Version
            } else {
                $version = "$Version-dev"
            }
        }
    }
    
    Write-Info "Version tag: $version"
    return $version
}

function Get-AvailableServices {
    $dockerDir = Join-Path $PWD "docker"
    if (-not (Test-Path $dockerDir)) {
        Write-Error-Custom "Docker directory not found: $dockerDir"
        return @()
    }
    
    $availableServices = @()
    foreach ($service in $TargetServices) {
        $servicePath = Join-Path $dockerDir $service
        $dockerfilePath = Join-Path $servicePath "Dockerfile"
        
        if ((Test-Path $servicePath) -and (Test-Path $dockerfilePath)) {
            $availableServices += $service
            Write-Verbose "Found service: $service"
        } else {
            Write-Verbose "Service not available: $service (missing directory or Dockerfile)"
        }
    }
    
    return $availableServices
}

function Show-AvailableServices {
    Write-Section "Available Services"
    $available = Get-AvailableServices
    
    if ($available.Count -eq 0) {
        Write-Warning-Custom "No services found in docker directory"
        return
    }
    
    Write-Host "Available services to build:" -ForegroundColor Cyan
    foreach ($service in $available) {
        $servicePath = Join-Path "docker" $service
        $hasBuildScript = Test-Path (Join-Path $servicePath "build.sh")
        $buildMethod = if ($hasBuildScript) { "build.sh" } else { "Dockerfile" }
        Write-Host "  • $service ($buildMethod)" -ForegroundColor White
    }
    
    Write-Host ""
    Write-Host "Usage examples:" -ForegroundColor Cyan
    Write-Host "  .\build-docker-services.ps1 -Services @('sui-node')" -ForegroundColor Gray
    Write-Host "  .\build-docker-services.ps1 -Services @('sui-node', 'sui-tools') -Verbose" -ForegroundColor Gray
    Write-Host "  .\build-docker-services.ps1 -Services @('sui-tools') -SkipPush" -ForegroundColor Gray
}

function Test-DockerAvailable {
    Write-Verbose "Checking if Docker is available..."
    try {
        $null = docker version 2>$null
        if ($LASTEXITCODE -eq 0) {
            Write-Success "Docker is available"
            return $true
        }
    } catch {
        Write-Error-Custom "Docker command failed: $($_.Exception.Message)"
    }
    
    Write-Error-Custom "Docker is not available or not running"
    return $false
}

function Invoke-DockerLogin {
    param([string]$RegistryUrl, [string]$Username, [string]$Password)
    
    Write-Section "Docker Registry Login"
    Write-Info "Logging into registry: $RegistryUrl"
    Write-Verbose "Username: $Username"
    
    # Check if password is provided
    if ($null -eq $Password -or $Password.Length -eq 0) {
        Write-Error-Custom "Registry password is required for login"
        Write-Info "Please provide a password using -RegistryPassword parameter as String"
        Write-Info "Or use -SkipPush to build without pushing to registry"
        return $false
    }
    # Check if username is provided
    if ($null -eq $Username -or $Username.Length -eq 0) {
        Write-Error-Custom "Registry username is required for login"
        Write-Info "Please provide a username using -RegistryUsername parameter as String"
        Write-Info "Or use -SkipPush to build without pushing to registry"
        return $false
    }
    # Check if url is provided
    if ($null -eq $RegistryUrl -or $RegistryUrl.Length -eq 0) {
        Write-Error-Custom "Registry url is required for login"
        Write-Info "Please provide a url using -RegistryUrl parameter as String"
        Write-Info "Or use -SkipPush to build without pushing to registry"
        return $false
    }
    
    try {
        # Use PowerShell to pipe password to docker login
        Write-Verbose "Attempting Docker login..."
        
        # Use Write-Output to pipe the password to docker login
        $loginResult = $Password | docker login $RegistryUrl -u $Username --password-stdin 2>&1

        if ($LASTEXITCODE -eq 0) {
            Write-Success "Successfully logged into Docker registry"
            return $true
        } else {
            Write-Error-Custom "Docker login failed: $loginResult"
            return $false
        }
    } catch {
        Write-Error-Custom "Docker login error: $($_.Exception.Message)"
        return $false
    }
}

function Build-DockerService {
    param(
        [string]$ServiceName,
        [string]$VersionTag,
        [string]$RegistryUrl,
        [string]$GitRevision,
        [string]$Branch
    )
    
    Write-Section "Building Service: $ServiceName"
    
    $dockerDir = Join-Path $PWD "docker"
    $servicePath = Join-Path $dockerDir $ServiceName
    $dockerfilePath = Join-Path $servicePath "Dockerfile"
    $buildScriptPath = Join-Path $servicePath "build.sh"
    
    # Validate service directory and Dockerfile
    if (-not (Test-Path $servicePath)) {
        Write-Error-Custom "Service directory not found: $servicePath"
        return $false
    }
    
    if (-not (Test-Path $dockerfilePath)) {
        Write-Error-Custom "Dockerfile not found: $dockerfilePath"
        return $false
    }
    
    # Set up image tags
    $imageTag = "$RegistryUrl/${ServiceName}:$VersionTag"
    $imageTagLatest = "$RegistryUrl/${ServiceName}:latest"
    
    Write-Info "Image tag: $imageTag"
    if ($Branch -eq "master" -or $Branch -eq "main") {
        Write-Info "Latest tag: $imageTagLatest"
    }
    
    # Change to service directory
    Push-Location $servicePath
    
    try {
        $buildDate = Get-Date -Format "yyyy-MM-dd" -AsUTC
        Write-Verbose "Build date: $buildDate"
        Write-Verbose "Git revision: $GitRevision"
        
        $buildSuccess = $false
        $env:DOCKER_BUILDKIT = $script:DockerBuildKit

        # Check if build.sh exists and use it
        if (Test-Path "build.sh") {
            Write-Info "Using build.sh script for $ServiceName"
            # Use the build script with parameters
            if ($IsWindows) {
                # On Windows, use WSL or Git Bash to run shell script
                Write-Verbose "Executing build script via bash"
                Write-Host "Running: bash ./build.sh -t $imageTag --build-arg BUILD_DATE=$buildDate --build-arg GIT_REVISION=$GitRevision --build-arg PROFILE=release" -ForegroundColor Gray
                
                # Capture and display output in real-time
                $process = Start-Process -FilePath "bash" -ArgumentList @("./build.sh", "-t", $imageTag, "--build-arg", "BUILD_DATE=$buildDate", "--build-arg", "GIT_REVISION=$GitRevision", "--build-arg", "PROFILE=release") -NoNewWindow -PassThru -Wait
                
                if ($process.ExitCode -ne 0) {
                    $LASTEXITCODE = $process.ExitCode
                }
            } else {
                # On Linux/macOS, make executable and run directly
                Write-Host "Running: ./build.sh -t $imageTag --build-arg BUILD_DATE=$buildDate --build-arg GIT_REVISION=$GitRevision --build-arg PROFILE=release" -ForegroundColor Gray
                chmod +x build.sh
                ./build.sh -t $imageTag `
                    --build-arg BUILD_DATE=$buildDate `
                    --build-arg GIT_REVISION=$GitRevision `
                    --build-arg PROFILE=release
            }
            
            if ($LASTEXITCODE -eq 0) {
                $buildSuccess = $true
            }
            
        } else {
            Write-Info "Using direct Docker build for $ServiceName"
            
            # Direct Dockerfile build
            Write-Verbose "Executing: docker build -f Dockerfile ../.. -t $imageTag --build-arg BUILD_DATE=$buildDate --build-arg GIT_REVISION=$GitRevision --build-arg PROFILE=release"
            
            docker build -f Dockerfile ../.. `
                -t $imageTag `
                --build-arg BUILD_DATE=$buildDate `
                --build-arg GIT_REVISION=$GitRevision `
                --build-arg PROFILE=release
            
            if ($LASTEXITCODE -eq 0) {
                $buildSuccess = $true
            }
        }
        
        if ($buildSuccess) {
            Write-Success "Successfully built $ServiceName"
            
            # Tag as latest for master/main branch
            if ($Branch -eq "master" -or $Branch -eq "main") {
                Write-Verbose "Tagging as latest: docker tag $imageTag $imageTagLatest"
                docker tag $imageTag $imageTagLatest
                if ($LASTEXITCODE -eq 0) {
                    $script:BuiltImages += $imageTagLatest
                    Write-Success "Tagged as latest: $imageTagLatest"
                }
            }
            
            $script:BuiltImages += $imageTag
            return $true
        } else {
            Write-Error-Custom "Build failed for $ServiceName"
            $script:FailedBuilds += $ServiceName
            return $false
        }
        
    } catch {
        Write-Error-Custom "Build error for ${ServiceName}: $($_.Exception.Message)"
        $script:FailedBuilds += $ServiceName
        return $false
    } finally {
        Pop-Location
    }
}

function Push-DockerImages {
    param([string[]]$ImageTags)
    
    if ($ImageTags.Count -eq 0) {
        Write-Warning-Custom "No images to push"
        return
    }
    
    Write-Section "Pushing Images to Registry"
    
    $pushSuccessCount = 0
    $pushFailCount = 0
    

    foreach ($imageTag in $ImageTags) {
        Write-Info "Pushing: $imageTag"
        Write-Verbose "Executing: docker push $imageTag"
        
        docker push $imageTag
        
        if ($LASTEXITCODE -eq 0) {
            Write-Success "Successfully pushed: $imageTag"
            $pushSuccessCount++
        } else {
            Write-Error-Custom "Failed to push: $imageTag"
            $pushFailCount++
        }
    }
    
    Write-Info "Push summary: $pushSuccessCount successful, $pushFailCount failed"
}

function Remove-DockerImages {
    param([string[]]$ImageTags)
    
    if ($ImageTags.Count -eq 0) {
        Write-Info "No images to clean up"
        return
    }
    
    Write-Section "Cleaning Up Local Images"
    
    foreach ($imageTag in $ImageTags) {
        Write-Verbose "Removing: $imageTag"
        docker rmi $imageTag 2>$null
        
        if ($LASTEXITCODE -eq 0) {
            Write-Success "Removed: $imageTag"
        } else {
            Write-Verbose "Could not remove: $imageTag (may not exist locally)"
        }
    }
}

function Show-BuildSummary {
    $endTime = Get-Date
    $duration = $endTime - $script:StartTime
    
    Write-Banner "Build Summary"
    
    Write-Host "Build Duration: " -NoNewline -ForegroundColor Cyan
    Write-Host "$([math]::Round($duration.TotalMinutes, 2)) minutes" -ForegroundColor White
    
    Write-Host "Built Images: " -NoNewline -ForegroundColor Cyan
    Write-Host "$($script:BuiltImages.Count)" -ForegroundColor Green
    
    if ($script:BuiltImages.Count -gt 0) {
        foreach ($image in $script:BuiltImages) {
            Write-Host "  ✅ $image" -ForegroundColor Green
        }
    }
    
    Write-Host "Failed Builds: " -NoNewline -ForegroundColor Cyan
    Write-Host "$($script:FailedBuilds.Count)" -ForegroundColor Red
    
    if ($script:FailedBuilds.Count -gt 0) {
        foreach ($service in $script:FailedBuilds) {
            Write-Host "  ❌ $service" -ForegroundColor Red
        }
    }
}

# Main execution
function Main {
    Write-Banner "Docker Service Builder"
    Write-Info "Starting Docker service build process..."
    Write-Verbose "PowerShell version: $($PSVersionTable.PSVersion)"
    Write-Verbose "Operating system: $($PSVersionTable.OS)"
    
    # Show available services if requested
    if ($ShowServices) {
        Show-AvailableServices
        return
    }
    
    # Validate password is provided when needed (not for ShowServices or SkipPush)
    if (-not $SkipPush -and ($null -eq $RegistryPassword -or $RegistryPassword.Length -eq 0)) {
        Write-Error-Custom "Registry password is required when pushing to registry"
        Write-Info "Please provide a password using -RegistryPassword parameter as SecureString"
        Write-Info "Examples:"
        Write-Info "  -RegistryPassword (Read-Host -AsSecureString -Prompt 'Password')"
        Write-Info "  -RegistryPassword (ConvertTo-SecureString 'yourpassword' -AsPlainText -Force)"
        Write-Info "Or use -SkipPush to build without pushing to registry"
        return
    }
    
    # Use all available services if none specified
    if ($Services.Count -eq 0) {
        $Services = Get-AvailableServices
        if ($Services.Count -eq 0) {
            Write-Error-Custom "No services found in docker directory"
            Show-AvailableServices
            return
        }
        Write-Info "No services specified, building all available target services: $($Services -join ', ')"
    }
    
    # Validate Docker availability
    if (-not (Test-DockerAvailable)) {
        Write-Error-Custom "Docker is required but not available. Please install Docker and ensure it's running."
        return
    }
    
    # Get git information
    $currentBranch = Get-GitBranch
    $gitRevision = Get-GitRevision
    $versionTag = Get-SemanticVersion -Version $Version -Branch $currentBranch
    
    Write-Info "Current branch: $currentBranch"
    Write-Info "Git revision: $gitRevision"
    Write-Info "Version tag: $versionTag"
    
    # Validate requested services
    $availableServices = Get-AvailableServices
    $validServices = @()
    
    foreach ($service in $Services) {
        if ($service -in $availableServices) {
            $validServices += $service
        } else {
            Write-Warning-Custom "Service '$service' not found or invalid. Skipping."
        }
    }
    
    if ($validServices.Count -eq 0) {
        Write-Error-Custom "No valid services specified"
        Show-AvailableServices
        return
    }
    
    Write-Info "Services to build: $($validServices -join ', ')"
    
    # Docker login (skip if SkipPush is specified)
    if (-not $SkipPush) {
        if (-not (Invoke-DockerLogin -RegistryUrl $RegistryUrl -Username $RegistryUsername -Password $RegistryPassword)) {
            Write-Error-Custom "Docker login failed. Cannot proceed with push."
            return
        }
    } else {
        Write-Info "Skipping Docker login (SkipPush specified)"
    }
    
    # Build services
    $buildSuccess = $true
    foreach ($service in $validServices) {
        $result = Build-DockerService -ServiceName $service -VersionTag $versionTag -RegistryUrl $RegistryUrl -GitRevision $gitRevision -Branch $currentBranch
        if (-not $result) {
            $buildSuccess = $false
        }
    }
    
    # Push images (if not skipped and builds were successful)
    if (-not $SkipPush -and $script:BuiltImages.Count -gt 0) {
        Push-DockerImages -ImageTags $script:BuiltImages
    } elseif ($SkipPush) {
        Write-Info "Skipping image push (SkipPush specified)"
    }
    
    # Cleanup (if not skipped)
    if (-not $SkipCleanup -and $script:BuiltImages.Count -gt 0) {
        Remove-DockerImages -ImageTags $script:BuiltImages
    } elseif ($SkipCleanup) {
        Write-Info "Skipping cleanup (SkipCleanup specified)"
    }
    
    # Show summary
    Show-BuildSummary
    
    # Exit with appropriate code
    if ($script:FailedBuilds.Count -eq 0) {
        Write-Success "All builds completed successfully!"
        exit 0
    } else {
        Write-Error-Custom "Some builds failed. Check the logs above."
        exit 1
    }
}

# Execute main function
Main 