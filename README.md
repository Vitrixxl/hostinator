# Caddy Deploy

Cette stack lance Caddy en HTTPS automatique et fournit une commande pour ajouter une app depuis un repo Git.

La commande clone ou pull le repo, lance son `docker compose up -d`, connecte le service exposé au réseau Docker de Caddy, écrit une entrée Caddy, puis recharge Caddy.

## Prérequis

- Un serveur avec Docker et Docker Compose v2.
- Les ports `80` et `443` ouverts vers le serveur.
- Un DNS qui pointe vers le serveur. Le plus simple est une entrée wildcard:

```text
*.example.com  A  <IP_DU_SERVEUR>
example.com    A  <IP_DU_SERVEUR>
```

## Installation

```bash
cp .env.example .env
```

Édite `.env` et remplace `BASE_DOMAIN=example.com` par ton domaine.

Puis lance Caddy:

```bash
./bin/site init
```

## Ajouter une app

Le repo doit contenir un fichier `compose.yaml`, `compose.yml`, `docker-compose.yaml` ou `docker-compose.yml`.

```bash
./bin/site add api https://github.com/org/api.git --port 3000
```

Avec `BASE_DOMAIN=example.com`, cette commande crée:

```text
https://api.example.com -> service Docker du repo sur le port 3000
```

Tu peux aussi passer le repo d'abord:

```bash
./bin/site add https://github.com/org/site.git blog --service web --port 8080
```

Si le compose n'a qu'un seul service, le script le choisit automatiquement. S'il y a un service `web`, il est choisi par défaut. Sinon, ajoute `--service <nom>`.

Si le port interne n'est pas détectable depuis l'image Docker, ajoute `--port <port>`.

## Mettre à jour

```bash
./bin/site update api
```

Cette commande fait un pull Git, relance `docker compose up -d`, reconnecte le conteneur au réseau Caddy si nécessaire, puis recharge Caddy.

## Supprimer

```bash
./bin/site remove api
```

Cette commande arrête le compose de l'app, supprime l'entrée Caddy et recharge Caddy.

## Fichiers utiles

- `compose.yaml`: stack Caddy.
- `Caddyfile`: importe toutes les entrées générées dans `caddy/sites/*.caddy`.
- `caddy/sites/`: configs Caddy générées par `./bin/site add`.
- `apps/`: repos Git clonés, ignoré par Git.
- `state/sites.tsv`: état utilisé par `update`, `remove` et `list`.

## Exemples

```bash
./bin/site add api https://github.com/acme/api.git --service app --port 3000
./bin/site add docs https://github.com/acme/docs.git --compose compose.prod.yaml --port 80
./bin/site list
```
