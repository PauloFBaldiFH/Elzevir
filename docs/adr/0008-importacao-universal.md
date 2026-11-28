# ADR 0008: importação universal pelo caminho de gravação existente

- **Status:** proposto no I0, aguardando revisão de Paulo.
- **Spec:** [importação universal](../specs/catalogo/importacao-universal.md).
- **Base:** [decisões de arquitetura](../arquitetura/04-decisoes.md),
  [definições](../arquitetura/05-definicoes-de-implementacao.md) e
  [ADR 0003](0003-bibframe-completo.md).

## Contexto

A importação ISO 2709 já possui leitura, mapeamento, casamento, origem
preservada e testes de ida e volta. Novos formatos e revisão antes da
gravação precisam usar essas garantias. A interface de revisão do Nexus
inspira o fluxo, mas seus padrões de Koha não pertencem ao núcleo BIBFRAME.

## Decisão

1. Detectar, ler, preparar, editar e apresentar a prévia são etapas em volta
   do importador atual, compartilhadas por web e CLI em `elzevir-app`.
2. Todo registro usa `RecordSink::submit` com `Option<Matching>` e os
   comandos `AddRecord`/`ImportRecord`. A escritora decide o casamento e
   grava pelas transações existentes; não há outro caminho de escrita.
3. MARC só para origem MARC: ISO 2709 conserva bytes sem edição; MARCXML,
   `.mrk` e metadados SQL do Koha são convertidos na borda. Edições MARC
   ocorrem antes do mapeamento. Identificadores `999 $c/$d` e `952 $9`
   não são removidos automaticamente.
4. Planilha produz BIBFRAME nativo, validado como na criação, com
   `source: None`. Cadastros são resolvidos por nome e identificador;
   não há conversão intermediária para MARC nem códigos artificiais.
5. Original e edições ficam fora do SQLite em lote privado e versionado
   até confirmação explícita. Prévia só consulta o pool de leitura.
   Sair da tela mantém o lote; cancelar ou concluir a confirmação o remove.
6. A tela fica na interface web da equipe, com permissão de catalogação,
   sessão e CSRF, dentro do servidor dono da escritora. A web depende de
   portas de aplicação, sem acessar `elzevir-marc` ou `elzevir-store`.

## Consequências

- Digest fixo em I1 precede qualquer generalização interna do pipeline.
  Leitores novos ganham limites, fuzzing e testes de equivalência.
- A prévia usa a preparação real, mas não reserva o catálogo; a escritora
  reavalia concorrência. O relatório informa recusas e resultados parciais.
- Antes de I8 é necessário resolver a ausência de exemplares nativos na
  entrada atual de `NewRecord`, sem desviar de `RecordSink` nem inventar
  códigos. Este ADR registra o requisito, não aprova alterar esse contrato.
- Mudança necessária em comportamento protegido ou schema exige pergunta
  no PR. Nenhuma migração ou dependência é introduzida por I0.

## Alternativas descartadas

- Converter planilha para MARC: introduz semântica de Koha na entrada nativa.
- Importador independente ou SQLite de staging: duplica gravação e casamento.
- Prévia por outro mapeador: permite divergência com a confirmação.
- Commit automático ao salvar edição: elimina a revisão explícita do lote.


## Evolução proposta em 2026-11-28

A v4 preserva o caminho bibliográfico da decisão 2. Leitores e autoridades
terão projeções próprias por portas de aplicação e comandos da mesma escritora.
A identidade institucional abrange todas as classes. O lote privado poderá
conter membros relacionados com um único plano aprovado. O primeiro incremento
propõe somente inventário em fluxo, com releitura controlada da fonte.
Qualquer persistência intermediária adicional exige orçamento medido e política
de retenção antes de sua introdução. O estudo e o roteiro registram estado e
critérios desta proposta. [Contrato v4](../specs/catalogo/importacao-universal-v4.md)
