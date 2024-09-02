# BOTM-web
A website which creates a Spotify playlist of your top songs every month.

# Configuration
Base configuration is set under `config/base.yaml`.
Further configuration for environment specific things is set under `config/dev.yaml` or `config/prod.yaml` based on how the `ENV` environment variable is set `local | dev | prod`.
Default `ENV` is `local`.

# Database
Connecting to the db using fly-cli
```
fly pg connect -a botm-web-db
```
and then connect to the `botm_web` db:
```
\c botm_web
```
