# Distribuição e instalação — A8/A9

Uma tag SemVer `vMAJOR.MINOR.PATCH` gera quatro arquivos: Linux musl x86_64 e
AArch64 e Windows MSVC x86_64 e ARM64. Cada pacote contém o binário, licença e
leia-me. A construção executa `--version` nativamente. O job de publicação
calcula `SHA256SUMS` de todos os pacotes e cria release em rascunho. O PR não
publica tag nem release. A instalação pública exige primeiro publicar uma
release validada; a versão `0.0.0` de desenvolvimento não é release estável.

Os instaladores Linux e PowerShell resolvem uma release uma única vez, fixam a
tag, baixam pacote e somas por HTTPS e verificam SHA-256 antes de extrair ou
executar. Recusam arquiteturas/gerenciadores não suportados, destinos ambíguos,
configurações existentes incompletas e extração insegura. `--dry-run`/`-Plan`
apresentam o plano sem baixar nem alterar arquivos. O administrador informa
nome/login; a senha é solicitada pelo próprio `elzevir init`, sem argumento.

Linux usa conta dedicada não-root e diretório local privado; Windows usa a
conta virtual do serviço com ACL explícita. A segunda execução preserva banco
e configuração, faz backup antes de substituir o binário e restaura o anterior
se o serviço novo não ficar saudável. Firewall e exclusão do Defender são
opções explícitas, nunca padrão. Dependências cloudflared/rclone são instaladas
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
