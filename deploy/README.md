# Build
docker build -f deploy/Dockerfile -t argus:latest .

# Run the dashboard (the public entry point)
docker run --rm -p 3000:3000 \
  -e ARGUS_NIM_KEY=nvapi-your-key-here \
  -e GITHUB_TOKEN=ghp-your-token-here \
  -v $(pwd)/docs:/app/docs \
  argus:latest /usr/local/bin/argus-dashboard

# Or run the unified CLI
docker run --rm -it \
  -e ARGUS_NIM_KEY=nvapi-your-key-here \
  argus:latest prompts

# Or run the lens cron
docker run --rm \
  -e ARGUS_NIM_KEY=nvapi-your-key-here \
  -v $(pwd)/docs:/app/docs \
  argus:latest /usr/local/bin/argus-lens \
    --org acme --mock-prs "acme/api#1,acme/web#2" \
    --output /app/docs/briefings/latest.md
