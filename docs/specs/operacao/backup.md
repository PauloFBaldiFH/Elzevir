# Backup, verificação e restauração — A5/P8

O backup é uma imagem SQLite consistente obtida pela API online a partir de
uma conexão de leitura existente. A conexão gravável do destino pertence
somente ao novo artefato temporário; nenhuma segunda escritora abre o banco
ativo. A cópia inclui o manifesto SHA-256 e versão de esquema, configuração
operacional, configuração local da biblioteca, operações e segredos conhecidos.
O índice é excluído e reconstruído na restauração.

O pacote tar comprimido inteiro é cifrado no formato age, por biblioteca, com uma
chave de recuperação aleatória de 256 bits em arquivo privado `backup.key`.
A chave nunca é incluída no pacote, em argumentos ou logs. O comando de
exportação grava somente um arquivo novo privado. Sem essa chave não é
possível recuperar a cópia; o painel oferece exportação explícita.

A publicação usa arquivo temporário e persistência sem sobrescrita. Diretórios,
links e arquivos preexistentes são recusados quando um destino novo é exigido.
O verificador autentica a cifra, limita entradas e tamanhos, rejeita nomes
arbitrários, links e travessia de diretórios no tar, compara hashes e executa
integrity_check no banco extraído. Esquemas mais novos são recusados.

Uma restauração exige a trava exclusiva da instituição e servidor parado.
Preserva a trava existente enquanto troca o banco e configurações, mantém
cópias de segurança para desfazer erros e reconstrói o índice antes de concluir.
Em uma instalação gerenciada, o cliente para/inicia o serviço e verifica a
saúde, retornando à cópia anterior se a inicialização falhar. Nenhum teste
opera o serviço real ou dados de uma instituição de produção.

Trabalhos do servidor são assíncronos, limitados a uma operação por instituição,
e o IPC aceita identificadores e opções tipadas, sem caminhos arbitrários de
destino. CLI/painel acompanham o mesmo trabalho. Fechar o painel não cancela
uma cópia iniciada. A parada do servidor interrompe trabalhos em pontos seguros.
Retenção remove apenas artefatos regulares reconhecidos e concluídos; mantém
pelo menos a cópia mais nova. Falhas são códigos estáticos sem conteúdo privado.

Origem: A5/P8 da análise do painel de Paulo; liberdade de implementação,
sem regra de paridade com Koha. Dependências novas de arquivo/cifra precisam
passar cargo-deny. Testes usam diretórios temporários e banco fictício.

O certificado de gestão da conta Cloudflare (`cf-cert.pem`) é uma exceção:
pertence ao operador elevado, fica inacessível ao daemon e não entra nos backups
da instituição. A recuperação inclui as credenciais limitadas do conector; para
administrar a conta/DNS depois de recuperar, o operador entra novamente na
Cloudflare. O manifesto recusa esse nome reservado, inclusive variações de caixa.

O preparo de arquivos usa diretório temporário privado do processo, fora do
diretório gravável pelo serviço. Arquivos auxiliares são abertos por capacidade
relativa à instituição: links não podem escapar desse diretório. Uma tabela de
autores externa deve ser movida para a instituição para participar do backup.
A verificação precisa de espaço temporário para a imagem SQLite descomprimida
e os auxiliares. Ela calcula os hashes dos objetos digitais durante a leitura.
Na restauração, o volume temporário também deve comportar os objetos
descompactados. O operador pode selecionar um volume privado maior por
`TMPDIR` (ou `TEMP` no Windows) para a restauração.

O catálogo de artefatos locais é separado do histórico de 64 trabalhos. Assim,
a retenção de até 365 cópias não perde a propriedade de arquivos antigos ao
truncar a tela de histórico. Arquivos sem registro permanecem intocados. Planos
de nuvem e seu catálogo de propriedade remota são transportados dentro da cifra;
após restauração, as agendas de nuvem ficam desativadas até revisão explícita.

Em restauração elevada, o pai dos dados deve ser controlado pelo administrador.
O preparo e os originais ficam em pasta privada irmã da instituição, fora de
sua árvore gravável pelo serviço. A configuração externa conserva seu original
no próprio diretório, para permitir volumes diferentes. A recuperação confere
uma cópia privada do diário antes de aceitar o marcador nos dados.

## Restauração pelo serviço gerenciado

O painel valida arquivo/chave antes de parar o serviço da instituição vinculada
ao executável. Depois restaura, reinicia e exige saúde. Se a restauração ou
a saúde falhar, para o serviço, repõe os arquivos originais preservados no
diário privado e reinicia a instalação anterior. Recusa vínculo com outra
instituição. Falha ao parar impede qualquer troca de dados. O comando offline
continua exigindo servidor parado e não controla o gerenciador de serviços.

## ACLs de segurança no Windows

Diretórios de preparo/recuperação do operador removem herança e concedem
controle total somente a SYSTEM e Administradores. A troca de proprietário
para Administradores usa uma chamada separada de `icacls /setowner`: o utilitário
não aceita combinar essa forma com as alterações `/grant` da DACL. Cada etapa
deve concluir com sucesso antes de mover qualquer arquivo da instituição.
Falha em uma etapa aborta a operação, sem fallback para permissões amplas.
Testes Windows verificam proprietário, herança e permissões de arquivos e pastas
temporários, além da restauração e rollback do banco fictício.

Antes de comparar destinos declarados, a restauração resolve o diretório da
instituição e o pai do alvo. Caminhos comuns, caminhos canônicos do Windows
(com prefixo verbatim) e aliases que resolvem para o mesmo diretório identificam
a mesma instituição. Alvos de outra instituição ou não declarados continuam
recusados antes de mover arquivos. A configuração restaurada aponta ao caminho
canônico do diretório de destino.

A proteção Windows substitui primeiro as permissões explícitas pela herança
padrão do pai (`icacls /reset`) e depois remove essa herança e aplica a lista
restrita. Remover somente a herança não elimina grants explícitos do criador
ou de contas anteriores. O proprietário é alterado apenas após proteger a DACL.

## Backup local com Zstandard

Novos pacotes mantêm o nome gerenciado `elzevir-<id>.age`. O escritor
comprime o tar com Zstandard antes de cifrar. O leitor identifica gzip ou
Zstandard dentro da cifra e autentica o final do fluxo. A janela máxima de
descompressão Zstandard é de 8 MiB. O manifesto v2 registra o escopo.
O manifesto v3 do backup completo inclui uma entrada por objeto digital
referenciado pela imagem SQLite. O leitor interpreta o manifesto v1 dos
arquivos históricos como instituição completa. A restauração completa exige
o escopo completo antes de parar o serviço.

No painel, N abre as opções de escopo e compressão. Instituição completa e
nível 3 são os padrões. Os níveis 1 e 7 priorizam velocidade e compactação,
respectivamente. Somente banco inclui o manifesto e a imagem SQLite.
O serviço verifica a cópia cifrada antes da publicação. O histórico identifica
o arquivo e a fase da execução. A estimativa de trabalho usa os tamanhos
observados do banco e WAL. O uso efetivo também depende do volume dos objetos
digitais, dos arquivos auxiliares e do volume escolhido para os temporários.
Os limites do manifesto e o procedimento de recuperação dos objetos estão na
[especificação do repositório](../repositorio/deposito.md#backup-completo-portátil-etapa-3).

Em Criar ou Exportar, F2 abre o seletor de pasta no host do operador.
F3 abre a pasta pessoal e F4 a pasta gerenciada. Use Tab entre lista, caminho
e ação. A confirmação da exportação cria a pasta informada quando necessário.
O serviço grava na pasta gerenciada e a exportação para o destino escolhido
é feita sob a identidade do operador, sem sobrescrever arquivo existente.

### Levar a cópia ao computador

Em Backups, Transferir parte da última cópia gerenciada que o catálogo identifica como verificada na criação. O operador informa o usuário e o endereço ou alias SSH que utiliza no próprio computador. O painel mostra o comando SCP para OpenSSH 9 ou posterior e só solicita a cópia para a área de transferência mediante ação explícita. O destino é a pasta atual do computador do operador, que deve estar vazia. Em um desktop local, Abrir pasta usa o gerenciador de arquivos. Arquivo cifrado e chave de recuperação seguem percursos separados.
