# Depósito digital — contrato da Etapa 1

Um depósito por instância, com página estável. A participação em coleções é
muitos-para-muitos e serve à navegação. A equipe de catalogação gerencia os
arquivos. Um arquivo administrativo sempre exige acesso de equipe.

A publicação exige instância visível no OPAC e versão vigente íntegra do
arquivo principal público. No acesso público a instância oculta e o rascunho
retornam 404; a página de depósito retirado retorna 410 e seus arquivos, 404.
A versão anterior só pode ser recuperada pela equipe. A retirada da versão
principal vigente requer retirada conjunta do depósito publicado. Nenhuma
retirada promove uma versão antiga. A republicação exige validar novamente
as condições de publicação.

O armazenamento instala bytes imutáveis, verifica SHA-256 e mantém a reserva
até a decisão do escritor. O escritor verifica a chave de operação antes da
revisão esperada e confirma metadados, auditoria e efeito de reindexação em
uma transação. A simulação em memória distingue interrupção antes da
instalação, antes do commit e perda da resposta depois do commit.

Leituras, uploads e backups ativos bloqueiam a manutenção. Temporários
abandonados são elegíveis após 24 h. Objetos definitivos sem referência são
elegíveis sete dias após a primeira constatação, com nova conferência sob
exclusão mútua. Versões retiradas e antigas retêm referência. A suíte do
módulo usa uma tese com PDF, planilha e termo administrativo; o caso de
deduplicação comprova que acesso depende do vínculo, mesmo com bytes iguais.

## Piloto offline (Etapa 2)

A CLI `elzevir repository --data-dir DIRETORIO` opera sob a trava exclusiva
`elzevir.lock` da instituição. O servidor e o backup offline já mantêm essa
trava. O backup online ocorre dentro do servidor. O operador local é
identificado na auditoria pelo ator reservado 0. O banco deve existir.
Os comandos e seus parâmetros aparecem em `elzevir repository --help`.
IDs públicos são números hexadecimais de 128 bits. Para repetir um upload,
o cliente conserva a chave de operação, o arquivo e a revisão esperada.

A migração 62 guarda apenas metadados. Os objetos ficam no diretório privado
`repository/objects/sha256/ab/cd/HASH`. O temporário fica no mesmo volume.
O padrão é 1 GiB por arquivo, 2 GiB livres como reserva e buffers de 256 KiB.
A manutenção inicia em simulação. Use `maintain --apply` para registrar
primeiras observações e remover arquivos elegíveis. O formato de backup desta
etapa cobre os metadados SQLite. A preservação dos bytes requer também uma
cópia do diretório de objetos sob exclusão mútua.

Antes de publicar um backup, o binário registra os hashes referenciados pela
imagem SQLite no índice privado `repository/maintenance/backup-pins.sqlite`.
A coleta consulta esse índice mesmo depois de restaurar um catálogo anterior.
Enquanto o arquivo de backup local existir, seus objetos ficam protegidos.
Após expirar o último backup local, a próxima constatação inicia a carência
de sete dias. Cópias exportadas para outra máquina precisam levar os objetos.

## Backup completo portátil (Etapa 3)

O backup completo inclui os objetos retidos por qualquer versão da imagem
SQLite, inclusive versões antigas, retiradas e restritas. Com objetos, o
manifesto usa o formato 3. Cada entrada `object-<sha256>` informa o tamanho
em bytes e o SHA-256 hexadecimal minúsculo. Dois arquivos com os mesmos bytes
ocupam uma única entrada. As permissões de cada vínculo continuam na imagem
SQLite. A cópia contém os documentos restritos e usa a criptografia age do
backup institucional.

`elzevir backup now` usa a imagem consistente do escritor online ou a trava
offline. Antes de publicar o arquivo, o sistema autentica a cópia, confere o
tamanho e o hash de cada entrada e compara o inventário com a imagem SQLite.
Um objeto ausente ou corrompido impede a publicação. O limite do piloto é
8.192 objetos distintos por backup completo, manifesto de até 2 MiB e
até 512 GiB somando as entradas descompactadas.

Para levar o acervo a outra instalação, use `backup export` para obter o
arquivo e `backup key` para exportar a chave de recuperação por um canal
separado. `backup verify --input ARQUIVO --recovery-key CHAVE` autentica e
verifica a cópia. Use `backup restore --input ARQUIVO --recovery-key CHAVE
--data-dir DESTINO --confirm-restore` com a instituição de destino
inicializada e parada. A restauração verifica e instala os objetos antes de
substituir o catálogo. Reutiliza objetos íntegros e recusa colisões com
conteúdo divergente. A instalação dos objetos usa links físicos no volume
privado do destino e preserva 2 GiB de espaço livre. Uma falha antes da troca
mantém o catálogo ativo. Objetos instalados sem referência seguem a carência
normal da coleta.

Backups antigos (formatos 1 e 2) e backups com escopo de banco continuam
legíveis. Nessas cópias, a verificação cobre a imagem SQLite e as demais
entradas declaradas. Para recuperar os documentos digitais, o operador
precisa restaurar separadamente o diretório de objetos da origem.

## Consulta pública no OPAC (Etapa 4a)

A ficha bibliográfica do OPAC oferece um link para o depósito publicado.
A página estável `/repository/{deposit}` apresenta o vínculo com a ficha
bibliográfica e os arquivos públicos vigentes e íntegros. Cada link de
download usa `/repository/{deposit}/files/{file}/versions/{seq}`. Os IDs
usam 32 dígitos hexadecimais minúsculos. GET e HEAD aplicam a mesma
autorização. A página retirada responde 410. Instâncias ocultas, rascunhos,
vínculos restritos e versões antigas respondem 404 ao visitante. Um objeto
autorizado indisponível responde 503 ao download.

A entrega usa anexo, valida tamanho e SHA-256 antes de enviar cabeçalhos e
transmite o arquivo inteiro em blocos de até 256 KiB. Uma requisição com
`Range` também passa pela autorização e recebe o arquivo inteiro (200).
O servidor admite até duas operações simultâneas de consulta à página do
depósito ou download por instituição e responde 503 quando ambas estão ocupadas. As respostas
pedem ao cliente e aos intermediários que consultem novamente a origem em
cada acesso. A CLI continua responsável pelo depósito e pela publicação.

## Descoberta no Tantivy (Etapa 4b)

O OPAC oferece uma entrada em `/repository`. Sem termos, ela lista depósitos
publicados de instâncias visíveis. O parâmetro `q` pesquisa os metadados
bibliográficos e nomes de coleções digitais. O filtro `collection` seleciona
um ID exato, inclusive quando duas coleções têm o mesmo nome. Cada resultado
leva ao depósito e às suas coleções. As páginas usam os limites do OPAC
(20 resultados, 256 caracteres de consulta e até 500 páginas).

O índice mantém um documento por instância. O consumidor existente de
`ReindexInstance` atualiza a presença do depósito e os nomes e IDs das coleções.
Rótulos de arquivos, conteúdo dos objetos, versões e dados administrativos ficam
fora dessa projeção. A troca de esquema do índice usa a reconstrução já existente
a partir da outbox. Entre o commit SQLite e a atualização do Tantivy pode faltar
um resultado novo. Antes da exibição, a consulta confere publicação, visibilidade
e participação na coleção no SQLite. Uma retirada pode encurtar temporariamente
a página de resultados.
