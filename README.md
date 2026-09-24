# Immich S3 Backup

Backs up an Immich instance (database dump + upload library) to Cloudflare R2 or any S3-compatible bucket, as password-protected zips.

It reads the same `DB_*` and `UPLOAD_LOCATION` variables as Immich, so it drops into an existing Immich `docker-compose` setup with no changes to Immich's own configuration.

## Setup

### 1. Add the service to Immich's `docker-compose.yml`

Paste this under `services:`:

```yaml
  immich-backup:
    container_name: immich_backup
    image: ghcr.io/iapetus-11/immich-cloudflare-backup:latest
    command: ["--forever"]
    env_file:
      - .env
    environment:
      # Paths inside this container, not on the host
      UPLOAD_LOCATION: /data
      BACKUP_HISTORY_FILE: /history/backup-history.sqlite
    volumes:
      - ${UPLOAD_LOCATION}:/data:ro
      - backup-history:/history
    depends_on:
      - database
    restart: unless-stopped
```

And under top-level `volumes:`:

```yaml
  backup-history:
```

### 2. Append to Immich's `.env`

```dotenv
# Immich Backup ########################################
AWS_ENDPOINT_URL=https://<cloudflare-account-id>.r2.cloudflarestorage.com  # Omit to use AWS S3
AWS_ACCESS_KEY_ID=
AWS_SECRET_ACCESS_KEY=
AWS_REGION=auto
AWS_BUCKET_NAME=my-immich-backup

BACKUP_ZIP_PASSWORD=
BACKUP_INTERVAL_DAYS=7
```

### 3. Start it

```sh
docker compose up -d immich-backup
```
