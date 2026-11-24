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
