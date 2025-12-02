# TeamTurbo CLI Usage Examples

## Login Examples

### Using Subdomain
The simplest way to connect to a TeamTurbo server:

```bash
# Connect to https://example.teamturbo.io
teamturbo login --domain example

# Connect to https://my-team.teamturbo.io
teamturbo login --domain my-team
```

### Using Full HTTPS URL
For custom domains or external servers:

```bash
# Connect to custom domain
teamturbo login --domain https://docs.example.com

# Connect to API endpoint
teamturbo login --domain https://api.example.com
```

### Using HTTP URL (Development)
For local development or testing:

```bash
# Connect to local development server
teamturbo login --domain http://localhost:3001

# Connect to local network server
teamturbo login --domain http://192.168.1.100:3000
```

### Interactive Mode
If you don't specify --domain, CLI will prompt you:

```bash
teamturbo login
# Prompts: Server domain or URL [example]:
# Enter: my-team
# Connects to: https://my-team.teamturbo.io
```

### Manual Token Mode
For environments without browser access:

```bash
# Interactive prompt for domain, then manual token input
teamturbo login --manual

# With domain specified
teamturbo login --domain example --manual
```

## Quick Start Workflow

```bash
# 1. Login to your server
teamturbo login --domain my-team

# 2. Initialize project with config URL
teamturbo init --config-url https://my-team.teamturbo.io/api/docuram/config/project/123/category/456

# 3. Check document status
teamturbo diff

# 4. Pull latest updates
teamturbo pull

# 5. Make changes to docs/...

# 6. Push changes
teamturbo push -m "Updated documentation"
```

## Domain Resolution Logic

The CLI automatically handles domain resolution:

| Input | Resolved URL |
|-------|--------------|
| `example` | `https://example.teamturbo.io` |
| `my-team` | `https://my-team.teamturbo.io` |
| `https://api.example.com` | `https://api.example.com` |
| `http://localhost:3000` | `http://localhost:3000` |
| `  my-team  ` (with spaces) | `https://my-team.teamturbo.io` |
| `https://example.com/` (trailing slash) | `https://example.com` |

## Common Scenarios

### Multi-Environment Setup

```bash
# Development
teamturbo login --domain http://localhost:3001

# Staging
teamturbo login --domain staging

# Production
teamturbo login --domain production
```

### Team Collaboration

```bash
# Each team member logs into team's server
teamturbo login --domain company-docs

# Pull latest changes
teamturbo pull

# Work on documents...

# Push updates
teamturbo push -m "Updated API documentation"
```
