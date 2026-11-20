# Distribuição e instalação — A8/A9

Uma tag SemVer, inclusive de pré-lançamento, deve coincidir com a versão do
workspace. A tag dispara a compilação nativa otimizada para Linux x86_64 GNU,
Linux musl x86_64 e AArch64 e Windows MSVC x86_64 e ARM64. Os arquivos
`elzevir-linux-x86_64.tar.gz` e `elzevir-windows-x86_64.zip` são os pacotes
de instalação rápida para x86_64. Os pacotes com nome versionado atendem
à instalação gerenciada e às atualizações.

Cada pacote contém licença, leia-me e o executável `elzevir`, que inclui o
servidor, a TUI `panel`, as migrações SQLite e os assets web. A validação
executa o binário extraído fora do checkout e verifica o catálogo, a busca
e a saúde do banco e do indexador. O job de publicação calcula `SHA256SUMS`
dos pacotes e dos instaladores, anexa os arquivos ao release e publica.
Tags com sufixo de pré-lançamento são publicadas como pré-releases.
A release também pode ser publicada pela interface web do GitHub, que cria a
tag: o job encontra a release publicada sem `SHA256SUMS` e anexa os pacotes.
Uma release que já tem `SHA256SUMS` nunca é alterada.

Os instaladores Linux e PowerShell consultam `releases/latest` e fixam a tag
resolvida antes de baixar pacote e somas por HTTPS. Como `releases/latest`
ignora pré-releases, enquanto o repositório não tiver uma release estável os
instaladores pedem à API do GitHub a release publicada mais nova
(`releases?per_page=1`), sem tag fixa no script.
`--version`/`-Version` permite fixar outra tag. O SHA-256 é verificado
antes da extração segura e da execução.

O executável (`elzevir update` e a seção Atualizações do painel) lê a lista
de releases (`releases?per_page=30`), descarta rascunhos e tags fora do
SemVer e escolhe a de maior precedência. Pré-releases só entram quando a
versão em execução já é um pré-lançamento: uma instalação alfa recebe a
alfa seguinte ou a estável, e uma instalação estável nunca é levada a uma
pré-release.

Sem opções de serviço, a instalação rápida grava o binário no PATH do
usuário, cria o diretório de dados local, verifica a versão e o subcomando
`panel` e exibe os caminhos e o próximo passo (`elzevir start`, que no
primeiro uso pede o nome da biblioteca e a senha do administrador). No
Linux, o script executado por `curl | bash` não altera o PATH do terminal
que o chamou; a mensagem final diz isso e mostra o caminho completo. Reinstalar preserva os dados da instituição.
Com as opções de serviço, o fluxo gerenciado exige o gerenciador suportado
e recusa destinos ambíguos e configurações incompletas. O administrador
informa nome e login. A senha é solicitada pelo próprio `elzevir init`.
`--dry-run`/`-Plan` apresentam o plano sem baixar nem alterar arquivos.

Linux usa conta dedicada não-root e diretório local privado; Windows usa a
conta virtual do serviço com ACL explícita. A segunda execução preserva banco
e configuração, faz backup antes de substituir o binário e restaura o anterior
se o serviço novo não ficar saudável. Firewall é opção explícita, nunca
padrão. O instalador PowerShell não cria exclusão do Defender nem altera
atalhos para elevação: são comportamentos típicos de malware e levam o
antivírus a bloquear o script. Quem quiser excluir o diretório de dados da
varredura faz isso manualmente. Dependências cloudflared/rclone são instaladas
somente por ação correspondente do painel. Nenhum teste instala serviço real.

Cada executável instalado fica vinculado a uma instituição e serviço por
registro privado no diretório de programas. Reutilizar o mesmo executável para
outra instituição é recusado; instalações adicionais usam diretório de programas
próprio. A atualização confere essa associação antes de parar o serviço, para
não migrar um banco ou substituir o binário de outra instituição.

A atualização mantém diário privado no diretório de programas antes de trocar
o executável. A última cópia fria fica em pasta privada do operador, fora dos
dados graváveis pelo serviço, e seu hash fica no diário. Uma recuperação
explícita conclui uma versão já saudável ou repõe o executável anterior e seu
banco. Arquivos divergentes dos hashes registrados são preservados e recusados.

Recuperação antes da troca do executável reinicia a versão anterior sem exigir
uma cópia fria ainda não criada. Depois de iniciada a troca, valida a cópia
privada antes de parar o serviço e registra a restauração do banco antes do
reinício. Repetir uma recuperação já restaurada não reaplica o backup sobre
escritas posteriores. Uma versão nova já saudável apenas conclui o diário;
uma versão anterior saudável durante rollback só conclui depois da fase de
restauração. Configuração explícita inválida ou de outra instituição é recusada.

Instâncias adicionais (`--id`/`-Id` diferente de `library`) usam executável em
diretório de programas próprio. A primeira instalação recusa sobrescrever um
executável existente sem configuração correspondente. Reexecução com banco e
configuração válidos, mas serviço ainda ausente, registra e inicia o serviço
antes de seguir pelo fluxo de atualização; não recria banco nem administrador.

## Atualização de instalação pessoal

`elzevir update` seleciona a release oficial, verifica o SHA-256 e
confirma a versão do executável. Com a instituição parada, mantém uma
cópia preventiva do SQLite pelo serviço de backup antes de publicar o
executável no mesmo volume. Instalações gerenciadas continuam usando
o fluxo de serviço. `--no-self-update` e instalações do WinGet delegam
a atualização ao instalador ou ao gerenciador de pacotes.
