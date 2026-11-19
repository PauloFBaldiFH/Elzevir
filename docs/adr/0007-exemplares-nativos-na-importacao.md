# ADR 0007 — Exemplares nativos na importação

- **Status:** aprovado para a implementação da I8.
- **Referências:** [Importação universal](0008-importacao-universal.md),
  [BIBFRAME completo](0003-bibframe-completo.md) e
  [grade de exemplares](../specs/catalogo/exemplares.md).

## Contexto

`NewRecord.items` é uma lista de `ItemData`, com códigos da origem MARC.
A grade usa `ItemFields`, com identificadores de unidade, localização,
coleção e política. `AddRecord` resolve códigos para todos os exemplares.
`ImportRecord` resolve os mesmos códigos nas operações de incluir e
substituir por código de barras, dentro da decisão da escritora.

## Proposta

1. Representar `NewRecord.items` por uma união tipada entre a lista
   MARC atual e a lista nativa. Cada exemplar nativo leva `ItemFields`
   e a descrição BIBFRAME do exemplar. O preparador resolve os nomes
   pelos cadastros da instituição e envia os identificadores resultantes.
   O registro de planilha usa `source: None`.
2. Manter `RecordSink::submit(NewRecord, Option<Matching>)` e os
   comandos `AddRecord` e `ImportRecord`. A escritora aplica a regra de
   casamento existente e revalida cadastros, vínculo com a unidade,
   descrição e código de barras na mesma gravação. A descrição da
   Instância e seus exemplares seguem os limites de transação dos
   respectivos comandos, inclusive o resultado por exemplar no casamento.
3. Preservar a forma JSON legada da lista MARC, inclusive quando vazia,
   para ler comandos históricos e reproduzir o digest I1. A forma nativa
   tem marcador próprio. A prova abrange o conteúdo de `audit_log.command`,
   além dos registros exportados.

## Alcance da revisão

A implementação adapta o tipo de `NewRecord.items` e seus construtores,
inclusive nos testes que constroem registros diretamente.
Em `matching.rs`, a mudança proposta se limita a encaminhar a persistência
dos exemplares pela variante de entrada. As buscas por chave, ações de
casamento, ordem e política de recusas têm o comportamento atual como
contrato. Na criação e no mapeamento MARC, a adaptação se limita ao
construtor de `items`. As regras de criação, edição e mapeamento seguem
o contrato atual, assim como o leitor MARC, o schema, as migrações e as
asserções existentes.

As provas para aceitar a implementação incluem o digest I1
`bf89658626085174` e a exportação byte a byte, leitura do formato legado
dos comandos, falha de cadastro entre a prévia e a confirmação,
reimportação ISBN sem duplicar e exportação MARC dos registros nativos.
A revalidação informa o exemplar e o código de barras exatos da recusa.
A preparação e a confirmação bloqueiam códigos de barras repetidos
entre os exemplares selecionados no lote tabular antes de submeter registros.
A escritora também bloqueia códigos repetidos no comando de um registro.
