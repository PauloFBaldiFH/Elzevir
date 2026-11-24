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
