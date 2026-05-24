# Rscholar
[English](./README.md) [中文](./README_zh.md)

[Example web](https://scholar.c76d3656e.sbs/) : http://c76d.abrdns.com/ 

Rscholar is a Rust-based academic literature search service with asynchronous task execution, multi-source retrieval, metadata enrichment, ranking filters, and CSV/BibTeX export.

This document reflects the current code in `src/`.

## What It Does

- Searches papers from multiple sources:
  - `openalex`
  - `semanticscholar`
  - `arxiv`
  - `pubmed`
  - `biorxiv`
  - `medrxiv`
- Runs as async task workflow (`POST /tasks` -> poll status -> download results)
- Performs optional enrichment:
  - Crossref DOI/abstract enrichment for papers missing DOI
  - Semantic Scholar batch lookup for abstracts/PDF URLs by DOI
- Applies ranking lookup via EasyScholar pool service (with SQLite cache)
- Applies ranking filters (`sciif`, `jci`, `sci`)
  - Preprint venues are not filtered out by ranking criteria
- Applies optional LLM relevance filtering when `content_help` is provided
- Exports:
  - Task JSON result
  - `results.csv`
  - BibTeX from task result (`/tasks/{id}/bibtex`)

## Usage

### 1. Configuration
Copy the example configuration and edit it to add your API keys:

```bash
cp config.example.toml config.toml
nano config.toml
```

**Required settings:**
- `[easyscholar]`: Add at least one valid EasyScholar key.
- `[llm]`: Add your LLM provider API key (e.g., SiliconFlow, AIPing, or BigModel).

**Note:** All search sources (OpenAlex, arXiv, PubMed, etc.) are enabled by default.

### 2. Startup
Run the start script to build and launch the service:

**Linux/macOS:**
```bash
./start.sh
```

**Windows PowerShell:**
```powershell
.\start.ps1
```

The script automatically:
- Installs frontend dependencies & builds the frontend
- Builds the Rust backend in release mode
- Starts the server at `http://localhost:3000`

## New/Important Current Behavior


- Keyword translation stage is built in:
  - If the input keyword is non-English, pipeline attempts LLM translation first
  - Downstream search and keyword expansion use the translated English keyword
  - On translation failure, pipeline falls back to original keyword
- Keyword expansion is based on the translated English keyword
- Source validation is strict:
  - Unknown source names in `source_include` return validation error
- Request-level `enable_llm_filter` was removed
  - Whether LLM filtering runs is decided by pipeline conditions (`content_help` + provider availability)
- Output directory naming format is:
  - `output/{timestamp}_{keyword}`

## Runtime Modes

- HTTP server mode (primary):
  - `Rscholar server --port 3000 --serve-static front/dist`
- CLI search mode (legacy/auxiliary):
  - `Rscholar search ...`


## API Overview

Public endpoints:

- `GET /health`
- `POST /tasks`
- `GET /tasks/{id}`
- `GET /tasks/{id}/download`
- `GET /tasks/{id}/bibtex`

Admin endpoints:

- Prefix: `/api/v1/admin/*`
- Auth: `X-API-Key` header (must be admin key)
- Includes key management, cache management, analytics, system status

See `docs/API.md` for full request/response details.

## Search Request Fields (`POST /tasks`)

Supported request JSON fields:

- `keyword` (required)
- `ylo`
- `enable_crossref`
- `sciif`
- `jci`
- `sci`
- `llm_strict_filter`
- `content_help`
- `source_include`
- `source_exclude`

Notes:

- Ranking filters require configured `easyscholar.keys`
- `source_include`/`source_exclude` are case-insensitive
- Supported source values:
  - `openalex`, `semanticscholar`, `arxiv`, `pubmed`, `biorxiv`, `medrxiv`

## Pipeline Stages (Server Task)

1. Keyword translation (LLM, if needed)
2. Keyword expansion (LLM, based on translated keyword)
3. Parallel source search
4. Merge + dedup
5. Crossref enrichment (missing DOI only)
6. Semantic Scholar DOI batch enrichment
7. Ranking lookup and assignment
8. Ranking filter (`sciif`/`jci`/`sci`, preprints exempt)
9. LLM relevance filter (only when `content_help` is non-empty)
10. Fallback handling (strict vs non-strict)
11. CSV save + analytics logging + task completion

## Project Layout

```text
src/
  cli/                     CLI commands (`search`, `server`, `init-admin`)
  db/                      SQLite schema + CRUD (tasks, keys, cache, analytics)
  llm/                     LLM providers, keyword expansion, keyword translation
  ranking/                 EasyScholar client pool + ranking service scheduler
  server/                  HTTP API (routes, handlers, pipeline, middleware, admin)
  sources/                 openalex / semanticscholar / arxiv / pubmed / xrxiv / crossref
  error.rs                 unified error type
  traffic.rs               traffic accounting helpers
  unified.rs               unified output structs (CLI path)
front/                     Vite frontend
docs/                      API and architecture docs
tests/                     integration and live tests
```

## Configuration

Main file: `config.toml`

Key sections:

- `[server]` host/port
- `[easyscholar]` API keys for ranking
- `[ranking]` scheduler/lease/key-health settings
- `[llm]` provider settings and fallback order
- `[search]` default ylo, source limits, enabled sources
- `[search.arxiv]`, `[search.pubmed]`, `[search.xrxiv]` source-specific settings

## Admin Key Bootstrap

Initialize first admin key:

```bash
cargo run -- init-admin --name Admin
```

Use returned key in header for admin routes:

```text
X-API-Key: <your-admin-key>
```

## Output and Persistence

- Task results are persisted in SQLite (`data/rscholar.db`)
- CSV output path per task:
  - `output/{timestamp}_{sanitized_keyword}/results.csv`
- Task memory cache is cleaned periodically (completed/failed tasks TTL)
- After restart:
  - task status can still be queried from DB
  - interrupted running tasks are recovered as failed

## Security Notes

- Admin API uses API key auth in middleware (`X-API-Key`)
- API keys are stored as HMAC-SHA256 hashes with server pepper
- Keep production keys out of version-controlled `config.toml`
- Prefer environment/secret management for API keys

## Development Notes

- Frontend build may require elevated process permissions in restricted environments
- Some integration/live tests are network dependent and may be slow on Windows

## Related Docs

- `docs/API.md`
- `docs/ARCHITECTURE.md`
- `docs/API_zh.md` (if maintained)
