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
