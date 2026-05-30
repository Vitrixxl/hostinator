# Hostinator

Hostinator déploie des projets Docker Compose derrière Caddy, puis les garde à jour via un webhook GitHub.

Le principe:

- Caddy tourne directement sur la machine avec `systemd`.
- L'API Rust `hostinator-webhook` tourne aussi avec `systemd`.
- Les repos sont clonés dans `$HOSTINATOR_HOME/repos`, par défaut dans ton home.
- Chaque app Docker Compose est exposée sur un port `127.0.0.1` stable.
- Caddy reverse-proxy le domaine public vers ce port local.
- Un push GitHub sur `main` ou `master` déclenche `git pull` puis `docker compose up -d --build`.

## Prérequis

- Un serveur avec Docker, Docker Compose v2, Git et Caddy installés.
- Les ports `80` et `443` ouverts vers le serveur.
- Un DNS qui pointe vers le serveur. Le plus simple est une entrée wildcard:

```text
*.example.com  A  <IP_DU_SERVEUR>
example.com    A  <IP_DU_SERVEUR>
```

## Configuration

```bash
cp .env.example .env
```

Édite `.env`:

```env
BASE_DOMAIN=example.com
HOSTINATOR_HOME=/home/vitrix/hostinator
HOSTINATOR_GITHUB_WEBHOOK_SECRET=un-secret-long
```

Le sous-domaine CI/CD est généré automatiquement sur:

```text
https://ci-cd.example.com/webhooks/github
```

Tu peux le changer avec `HOSTINATOR_CICD_SUBDOMAIN` ou `HOSTINATOR_CICD_DOMAIN`.

## Installation système

```bash
cargo build --release

sudo install -m 0755 bin/hostinator /usr/local/bin/hostinator
sudo install -m 0755 target/release/hostinator-webhook /usr/local/bin/hostinator-webhook

sudo mkdir -p /etc/hostinator
sudo install -m 0600 systemd/hostinator.env.example /etc/hostinator/hostinator.env
sudo install -m 0644 systemd/hostinator-webhook.service /etc/systemd/system/hostinator-webhook.service
```

Édite `/etc/hostinator/hostinator.env`, puis initialise Caddy:

```bash
hostinator init
sudo systemctl daemon-reload
sudo systemctl enable --now hostinator-webhook
```

`hostinator init` crée l'import Caddy si le fichier `/etc/caddy/Caddyfile` n'existe pas, crée `/etc/caddy/sites`, et génère le vhost `ci-cd.<BASE_DOMAIN>`.

## Ajouter une app

Le repo doit contenir un fichier `compose.yaml`, `compose.yml`, `docker-compose.yaml` ou `docker-compose.yml`.

```bash
hostinator add api https://github.com/org/api.git --port 3000
```

Avec `BASE_DOMAIN=example.com`, cette commande crée:

```text
https://api.example.com -> 127.0.0.1:<port-alloué>
```

Tu peux aussi passer le repo d'abord:

```bash
hostinator add https://github.com/org/site.git blog --service web --port 8080
```

Si le compose n'a qu'un seul service, Hostinator le choisit automatiquement. S'il y a un service `web`, il est choisi par défaut. Sinon, ajoute `--service <nom>`.

Si le port interne n'est pas détectable depuis l'image Docker, ajoute `--port <port>`.

## Webhook GitHub

Dans GitHub, ajoute un webhook sur le repo déployé:

```text
Payload URL: https://ci-cd.example.com/webhooks/github
Content type: application/json
Secret: la valeur de HOSTINATOR_GITHUB_WEBHOOK_SECRET
Events: Just the push event
```

À chaque push sur `main` ou `master`, l'API Rust lance:

```bash
hostinator webhook-update --repo <repo> --branch <branche>
```

Hostinator retrouve les sites qui utilisent ce repo, fait un `git pull --ff-only`, rebuild le Docker Compose avec `--build`, puis recharge Caddy.

## Mettre à jour manuellement

```bash
hostinator update api
```

Par défaut, `update` rebuild avec `docker compose up -d --build`. Pour pull sans rebuild forcé:

```bash
hostinator update api --no-build
```

## Supprimer

```bash
hostinator remove api
```

Cette commande arrête le compose de l'app, supprime l'entrée Caddy générée et recharge Caddy.

## CI du projet

Le workflow `.github/workflows/ci.yml` vérifie:

- syntaxe Bash;
- Shellcheck;
- `cargo fmt --check`;
- `cargo check --locked`;
- build release du webhook.

## Fichiers utiles

- `bin/hostinator`: CLI principal.
- `bin/site`: wrapper de compatibilité vers `bin/hostinator`.
- `src/main.rs`: API Rust du webhook GitHub.
- `systemd/hostinator-webhook.service`: service systemd de l'API Rust.
- `systemd/hostinator.env.example`: environnement système à copier dans `/etc/hostinator/hostinator.env`.
- `$HOSTINATOR_HOME/repos`: repos Git clonés.
- `$HOSTINATOR_HOME/state/sites.tsv`: état utilisé par `update`, `remove`, `list` et le webhook.
- `$HOSTINATOR_HOME/run/compose-overrides`: overrides Docker Compose générés pour exposer les apps sur localhost.
