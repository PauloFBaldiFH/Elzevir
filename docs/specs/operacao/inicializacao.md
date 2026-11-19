# Inicialização da instituição — A2

## 1. Comportamento

1. `elzevir init --name "Minha biblioteca"` cria o primeiro administrador
   (`admin`), o banco, o índice e a configuração no diretório do usuário.
   `--config`, `--data-dir`, `--login`, `--listen` e `--insecure-cookies`
   selecionam destinos e preferências. Com diretório explícito, o arquivo
   de configuração padrão é `elzevir.toml` no diretório atual.
2. Destinos explícitos exigem pais existentes e o caminho final novo. O
   caminho padrão aceita a pasta vazia criada pelo instalador. Configuração,
   dados existentes e links são preservados em todas as tentativas.
3. O nome tem 1 a 200 caracteres após remover espaços nas extremidades e
   não contém controles. O login tem 1 a 64 caracteres ASCII: letras, dígitos,
   `_`, `-` ou `.`. O nome fica em `[institution].name`; a pasta de dados é
   gravada como caminho absoluto no TOML, usando o serializador da A1.
4. A senha tem ao menos 8 caracteres (mesma regra de `staff add`) e no máximo
   1.024 bytes. Em terminal interativo, é digitada sem eco e confirmada.
   `--password-stdin` lê uma linha sem prompt e tem precedência sobre
   `ELZEVIR_STAFF_PASSWORD`; sem esse sinalizador, a variável é usada quando
   presente. Sem terminal e sem essas fontes, o comando falha. O sinalizador
   de stdin é recusado em terminal interativo para não ecoar a senha.
5. Senha inválida, confirmação diferente, caminhos inválidos ou configuração
   não serializável falham antes de criar dados. Não há senha por argumento.
   Senhas e hashes não aparecem nas mensagens ou no TOML.
6. O administrador tem permissões de equipe, administração e catalogação.
   Sua criação passa pelo comando `StaffCommand::Create` e pela escritora,
   com Argon2id e auditoria existentes. Perfis são opcionais.
7. No Linux, o tipo de filesystem é consultado via `statfs`; somente tipos
   locais reconhecidos são aceitos. No macOS, `statfs` exige a marca de volume
   local; no Windows, o destino precisa corresponder a um volume local reconhecido.
   Rede e resultados desconhecidos
   são recusados; não há opção para ignorar a verificação.
8. Em Unix, a pasta nasce com modo 0700 e o TOML temporário com 0600. Em
   Windows, as permissões são herdadas; ACLs específicas de serviço ficam para etapa própria.
9. A escritora termina e o checkpoint do banco é confirmado após criar o
   administrador, antes de publicar o TOML. Uma falha nessa etapa impede
   a publicação da configuração.
   O arquivo é preparado no seu diretório, sincronizado e publicado sem
   sobrescrever destino existente, inclusive em uma corrida entre processos.
   Em falha normal, somente a pasta recém-criada por esta execução é removida;
   falha na limpeza é informada. Arquivos preexistentes nunca são removidos.
10. A configuração operacional permanece separada do arquivo de domínio
    `DATA_DIR/elzevir.toml` usado pela tabela de autores. `init` não grava
    configuração de domínio nem altera `--iri-base` da A1.
11. Após sucesso, o comando informa o caminho da configuração. O usuário pode
    executar `elzevir serve --config CAMINHO`; não instala nem inicia serviços.

## 2. Origem no Koha

Não porta comportamento do Koha. Base: doc 04, §6/§7 e etapa A2 da análise
do painel fornecida por Paulo.

## 3. Paridade

Livre. `rpassword` (Apache-2.0) lê a senha sem eco. `tempfile`, já usado pelo
workspace, publica o arquivo sem sobrescrita; `toml` também escreve a configuração.
`rustix` (já transitivo; MIT/Apache-2.0) oferece statfs seguro no Linux/macOS;
`sysinfo` (MIT, recurso disk) inspeciona os volumes no Windows.

## 4. Casos de borda

- O administrador autentica com a senha escolhida e tem as três permissões.
- A configuração gerada é lida pela A1 e inicia o catálogo.
- Repetição, arquivo, pasta ou link preexistentes preservam conteúdo e senha.
- Nome, login, senha e tamanho de entrada inválidos não criam instalação.
- Publicação concorrente do TOML falha sem sobrescrever e desfaz os dados novos.
- UTF-8, espaços e aspas nos caminhos são serializados corretamente.
- Entrada interativa oculta ambas as digitações; Ctrl+C não cria instalação.

## 5. Desvios

Nenhum: não há área de paridade.

## 6. Fora do escopo

Instalação de serviços do sistema, edição de instalação
existente e recuperação automática após queda de energia. Banco e TOML em
destinos separados não são uma única transação de filesystem: se o processo
for encerrado abruptamente, pode restar uma pasta incompleta, que a próxima
execução preserva e recusa. A verificação de volume não identifica toda camada
de armazenamento por baixo de overlay nem pastas sincronizadas na nuvem;
não use NFS/SMB, OneDrive ou equivalentes para os dados SQLite.
## Pré-verificação de porta pelo instalador

`init --check-port` verifica se o endereço solicitado pode ser vinculado antes
de pedir a senha ou criar dados. Mantém a porta reservada durante a criação e
a libera ao terminar. A verificação é opcional para usos offline de init;
os instaladores a solicitam na primeira instalação.

## Primeiro uso

O modo padrão usa `$XDG_DATA_HOME/elzevir` ou `~/.local/share/elzevir`
no Linux e macOS e `%LOCALAPPDATA%\Elzevir\data` no Windows. A
configuração operacional padrão fica em `server.toml` nessa pasta.
As opções explícitas continuam atendendo às instalações personalizadas.

`init` em terminal pede o nome, a senha oculta com confirmação e, se
desejado, um perfil inicial público, escolar ou universitário. O perfil
usa os comandos existentes de unidade e política de circulação. O
vocabulário de materiais continua vindo das listas oficiais de BIBFRAME.
A instalação prepara SQLite e Tantivy antes de publicar a configuração.

`serve`, `panel` e `start` oferecem o assistente no primeiro uso do
diretório padrão em terminal. `start` executa o servidor e o painel na
mesma instância e encerra o servidor de forma ordenada ao sair do painel.
`--lang` seleciona o idioma explicitamente. Locales em português selecionam
pt-BR por padrão.
