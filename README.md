# Elzevir
A modern and lightweight library system

## Primeira inicialização

Com o binário disponível no terminal, execute em uma pasta da instalação:

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
descreve as opções e os limites desta etapa. Instaladores e registro como
serviço serão entregues separadamente.

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
