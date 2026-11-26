# Elzevir
A modern and lightweight library system

## Instalação Rápida

**Linux (Bash)**

```bash
curl -fsSL https://raw.githubusercontent.com/PauloFBaldiFH/Elzevir/main/install.sh | bash
```

**Windows (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/PauloFBaldiFH/Elzevir/main/install.ps1 -OutFile "$env:TEMP\elzevir-install.ps1"
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:TEMP\elzevir-install.ps1"
```

Se o antivírus bloquear o script, não desative a proteção. Instale sem
script: baixe `elzevir-windows-x86_64.zip` e `SHA256SUMS` da
[release mais recente](https://github.com/PauloFBaldiFH/Elzevir/releases),
confira o hash com `Get-FileHash elzevir-windows-x86_64.zip` (deve ser
igual à linha do arquivo em `SHA256SUMS`), extraia e rode
`.\elzevir.exe start` na pasta extraída.

Os instaladores verificam o SHA-256 do pacote e adicionam `elzevir` ao PATH
do usuário. Eles consultam a release estável mais recente. Enquanto não
houver release estável, usam o pré-lançamento mais novo. O terminal em que
o instalador rodou no Linux ainda não conhece o comando: rode
`source ~/.bashrc` ou abra outro terminal, como o instalador indica.

Abra o Elzevir pelo terminal.

```sh
elzevir
```

Na primeira execução, o painel mostra a pasta selecionada. Você pode explorar
as abas ou sair. Para criar a biblioteca, abra **Ajustes → Configurar biblioteca**,
escolha o nome, o usuário administrador, a senha e o perfil inicial. Ao terminar,
abra **Serviço → Iniciar** para atender o catálogo.

Nas próximas aberturas, `elzevir` e `elzevir start` iniciam automaticamente
uma instalação pessoal saudável que esteja parada. O servidor continua ativo
quando o painel fecha. Use `elzevir stop` para encerrá-lo. Também é possível
usar `elzevir serve` e `elzevir panel` em terminais separados ou
`elzevir panel --demo` para explorar com dados simulados.

## Primeira inicialização

```sh
elzevir init
elzevir start --lang pt-BR
```

O diretório padrão é `$XDG_DATA_HOME/elzevir` ou
`~/.local/share/elzevir` no Linux e macOS, e
`%LOCALAPPDATA%\Elzevir\data` no Windows. A configuração operacional
fica em `server.toml` nesse diretório. `--data-dir` e `--config`
aceitam destinos explícitos. `--lang` sobrescreve o idioma do sistema.

Para automação, passe `--name` e leia a senha de
`ELZEVIR_STAFF_PASSWORD` ou use `--password-stdin` com entrada
redirecionada. `--profile public`, `school` e `university` são opcionais.

```sh
elzevir init --name "Minha biblioteca" --password-stdin < senha.txt
elzevir serve
```

Para atualizar uma instalação pessoal, encerre o servidor e execute
`elzevir update`. O comando verifica o SHA-256 do pacote oficial, faz
backup preventivo e substitui o executável. Use `update --data-dir CAMINHO`
para atualizar uma instalação pessoal com dados fora do padrão. O WinGet continua responsável
pelas suas instalações. Use `elzevir update --no-self-update` para
recusar a substituição. Consulte o [guia de operação](docs/operacao.md)
e a [especificação de inicialização](docs/specs/operacao/inicializacao.md).

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
elzevir panel --lang pt-BR
```

O modo demo usa dados simulados. O modo conectado consulta a instituição
selecionada e mostra as ações disponíveis em cada seção. As operações que
controlam serviços e arquivos de instalação exigem privilégios do operador.
Veja o [guia e a situação do roadmap operacional](docs/operacao.md).
