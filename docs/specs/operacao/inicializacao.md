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

`init` em terminal pede o nome da biblioteca, o login, a senha oculta com confirmação e, se
desejado, um perfil inicial público, escolar ou universitário. O perfil
usa os comandos existentes de unidade e política de circulação. O
vocabulário de materiais continua vindo das listas oficiais de BIBFRAME.
A instalação prepara SQLite e Tantivy antes de publicar a configuração.

`panel` e `start` abrem a visão geral. Na pasta pessoal nova, o operador
inicia o assistente em Configurações. O comando explícito `serve` mantém
o percurso interativo próprio. Em instituição pessoal saudável, `start`
inicia o processo separado `elzevir background` e abre o painel ligado a
ele. Fechar o painel preserva o servidor.
`elzevir background` inicia o servidor da instituição padrão sem abrir
nada e não faz nada se ele já estiver rodando. No login, o servidor sobe
pela bandeja: no Linux, pela entrada em `~/.config/autostart` criada por
`elzevir desktop install`; no Windows, pelo atalho na pasta Inicializar
para `elzevir-tray.exe`. Instalações sem esse programa usam o atalho para
`elzevir background`.
`elzevir stop` pede pelo canal administrativo local a parada ordenada.

`elzevir tray` é o ícone de bandeja. Ao abrir, inicia o servidor se ele
estiver parado (mesmo onde não há bandeja) e mostra o estado: rodando,
rodando com problemas, parado, iniciando, parando, sem resposta ou ainda
não configurado. O menu oferece só o que o estado permite: abrir o painel
(também com clique simples), abrir o catálogo e a interface da equipe no
navegador, iniciar ou parar o servidor e fechar o ícone. No Linux usa o
protocolo StatusNotifierItem (KDE, XFCE, Cinnamon, MATE, LXQt; GNOME só
com a extensão AppIndicator). Sem bandeja, o comando sai com aviso
depois de iniciar o servidor. O painel abre pelo atalho do menu
(`gio launch`), que usa o terminal preferido do desktop, ou pelo primeiro
terminal conhecido encontrado. No login do Linux, a entrada de autoinício
roda `elzevir tray`.
No Windows o ícone fica na área de notificação (Win32, via `tray-icon` e
`winit`). Todo programa de console ganha uma janela preta no Windows, por
isso o login abre `elzevir-tray.exe`: um programa sem console que só
inicia `elzevir.exe tray` com o console oculto. O painel abre num console
novo (no Windows Terminal, quando ele é o terminal padrão); catálogo e
equipe abrem no navegador padrão.
A bandeja nunca roda o assistente de primeiro uso. Se a instituição ainda
não existe, mostra "não configurado" e passa a mostrar o servidor assim que
o painel a cria.
A atualização pessoal para o servidor em segundo plano antes de trocar o
executável e o inicia de novo depois, mesmo se a atualização falhar. No
Windows, um programa em execução (a bandeja, ou o painel que pediu a
atualização) não pode ser substituído, mas pode ser renomeado: a
atualização move o executável para `elzevir.old`, coloca o novo no lugar e
volta o antigo se isso falhar. A bandeja aberta continua na versão antiga
até o próximo login. O `elzevir.old` da atualização anterior é apagado na
seguinte; se ainda estiver em uso, a atualização recusa e pede para fechar
a bandeja e o painel.
`elzevir` sem subcomando equivale a `elzevir start`, para que o uso diário
não exija lembrar comandos.
Se a porta configurada estiver ocupada, `start` (e o primeiro uso) abre
na TUI o aviso "Porta em uso" com a próxima porta livre já preenchida.
Enter aceita; outra porta pode ser digitada (1024 a 65535). A porta
escolhida é testada antes de fechar o aviso e gravada no `server.toml`,
exceto quando veio de `--listen`, que nunca reescreve o arquivo. Esc
cancela. Sem terminal interativo, o erro pede `--listen`.
`--lang` seleciona o idioma explicitamente. Locales em português selecionam
pt-BR por padrão.

## Entrada pelo painel

O lançamento interativo abre o painel na instalação nova ou incompleta. A inspeção distingue destino novo, dados preexistentes sem configuração, configuração ilegível ou inválida e configuração sem banco. Abrir, navegar e sair nessa situação não cria dados e não inicia o servidor. Configurações abre o assistente por escolha explícita apenas em destino novo compatível com o inicializador. Cancelar volta ao painel. Em banco parado, a consulta à permissão de administração exige a trava existente e uma base sem diário pendente. Uma leitura imutável do SQLite preserva os arquivos da instituição. Para dados legados com servidor ativo identificado, Configurações oferece abrir a sessão operacional existente. Instalações pessoais configuradas preservam o início automático quando a parada está confirmada. A edição de instalações existentes e a recuperação de um administrador exigem autoridade e verificação próprias; ausência de TOML não prova ausência de administrador.
