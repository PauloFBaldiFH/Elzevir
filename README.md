# Elzevir
A modern and lightweight library system

## Instalação Rápida

**Linux (Bash)**

```bash
curl -fsSL https://raw.githubusercontent.com/PauloFBaldiFH/Elzevir/main/install.sh | bash
```

**Windows (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/PauloFBaldiFH/Elzevir/main/install.ps1 | iex
```

Os instaladores verificam o SHA-256 do pacote e adicionam `elzevir` ao PATH
do usuário. Eles consultam a release estável mais recente. Enquanto ela
estiver indisponível, usam o pré-lançamento `v0.1.0-alpha.1`.

Para iniciar o servidor no mesmo terminal:

```bash
export PATH="$HOME/.local/bin:$PATH"
elzevir serve --data-dir "${XDG_DATA_HOME:-$HOME/.local/share}/elzevir"
```

```powershell
elzevir serve --data-dir "$env:LOCALAPPDATA\Elzevir\data"
```

Abra o catálogo em `http://127.0.0.1:8080`. Em outro terminal, abra o
painel Ratatui com a mesma pasta de dados, ou use `--demo` para explorar
o painel com dados simulados.

```bash
elzevir panel --data-dir "${XDG_DATA_HOME:-$HOME/.local/share}/elzevir" --lang pt-BR
```

```powershell
elzevir panel --data-dir "$env:LOCALAPPDATA\Elzevir\data" --lang pt-BR
```

## Primeira inicialização

Para criar uma instituição com administrador e configuração própria, execute
em uma pasta local com destinos novos:

```sh
elzevir init --name "Minha biblioteca"
elzevir serve --config elzevir.toml
```

`init` pede e confirma a senha sem mostrá-la, cria o administrador `admin`,
a pasta `data-local` e a configuração. Recusa destinos existentes e volumes
de rede. O servidor usa `http://127.0.0.1:8080` por padrão. Em desenvolvimento
com HTTP local, use `serve --config elzevir.toml --insecure-cookies`.

Para automação, use `init --password-stdin` com entrada redirecionada ou a
variável `ELZEVIR_STAFF_PASSWORD`. A [especificação da inicialização](docs/specs/operacao/inicializacao.md)
descreve as opções e os limites desta etapa. Consulte o [guia de operação](docs/operacao.md)
para instaladores Linux/Windows, serviço, painel, backups e atualizações.

## Saúde local

`serve` anuncia uma URL de saúde em `127.0.0.1`, numa porta escolhida pelo
sistema. Ela retorna JSON e HTTP 200 quando banco e indexador estão prontos;
caso contrário, retorna 503. A rota não é exposta pelo endereço do catálogo.
Veja o [contrato de saúde local](docs/specs/operacao/saude-local.md).

## Consultar o estado

```sh
elzevir status --config elzevir.toml
elzevir status --config elzevir.toml --json
```

A consulta informa `running`, `degraded`, `stopped` ou `unavailable`, sem abrir
SQLite nem criar arquivos. Retorna código zero somente quando o servidor está
pronto. Veja o [contrato do status local](docs/specs/operacao/status-local.md).

## Logs de operação

`serve` grava eventos JSON em `DATA_DIR/logs/elzevir.DATA.jsonl`, com data UTC,
rotação diária e retenção configurada para sete arquivos. Os eventos de início,
parada e indexação omitem o conteúdo das requisições. Consulte o
[contrato dos logs operacionais](docs/specs/operacao/logs-operacionais.md).

## Painel de administração

```sh
elzevir panel --demo --lang pt-BR
elzevir panel --config elzevir.toml --lang pt-BR
```

O modo demo usa dados simulados. O modo conectado consulta a instituição
selecionada e mostra as ações disponíveis em cada seção. As operações que
controlam serviços e arquivos de instalação exigem privilégios do operador.
Veja o [guia e a situação do roadmap operacional](docs/operacao.md).
