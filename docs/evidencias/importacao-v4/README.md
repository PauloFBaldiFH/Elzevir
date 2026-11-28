# Estudo de decisão da importação universal

**Revisão de 28 de novembro de 2026.** Base `be79b410` da main, PR #304.
A identidade de origem e a preservação do PR #228 (`9b330865`) e o perfil
de exemplares 949 do #229 (`369d6a48`) já pertencem à base. A investigação
0A e o contrato 0B separam resultados reproduzidos nesta revisão das
capacidades futuras. [Contrato e oráculos](../../specs/catalogo/importacao-universal-v4.md) ·
[Ordem de implementação](../../arquitetura/03-estrategia-reconstrucao.md)

## Decisão

Começar por **inventário privado em fluxo e vínculos com evidência**. Uma
coluna que contém todos os IDs de outra pode representar um leitor, um operador
ou uma Instância. No ensaio abaixo, aceitar inclusão isolada sugeriu três
relações falsas em sete propostas. O operador deve poder consultar qual
evidência sustenta o papel, quantos vínculos fecham e quais ficam pendentes.
O primeiro resultado de produto proposto é uma prévia de tabelas PostgreSQL
COPY na CLI e no Staff. A amostra com 11.561 descrições permite conferir
também a diferença entre linhas lidas e descrições catalogáveis.

## Anatomia do corpus e qualificação

O [manifesto](corpus.tsv) identifica os 32 arquivos rastreados por caminho,
SHA-256, tamanho e medida. Dois são auxiliares (mandato e imagem). O
[inventário Evergreen](evergreen.tsv) identifica cada um dos 96 membros
regulares, e o [inventário SQL](tabelas.tsv) registra declarações e contagens
por tabela e conjunto. A unidade de versão de todo arquivo sem identificação
de release comprovada é seu digest nesta revisão. As versões do projeto
produtor não se deduzem de nomes de diretório.

As contagens SQL são leituras lexicais de DDL, listas VALUES e linhas COPY,
sem conexão com banco. Expressões e instruções geradoras ficam sem avaliação.
Um zero com DDL indica ausência de linhas nas formas contadas. Em scripts
com SELECT, funções ou criação dinâmica, zero pode significar que a quantidade
gerada é desconhecida. A coluna final conta INSERTs reconhecidos que ficaram
sem cardinalidade. Os dados do PGDMP foram contados pela saída textual de
`pg_restore` 17.12, sem servidor nem restauração. Para ISO 2709 mediram-se
comprimento, terminador e tipo de líder por registro. Para XML contou-se por
elemento. Isso deixa validação de conteúdo e equivalência de projeção para
testes específicos.

**Níveis.** 1 reconhece transporte, 2 inventaria estrutura, 3 extrai unidades,
4 explica a semântica na prévia, 5 grava e reapresenta com integridade,
6 qualifica operação numa versão determinada. A coluna de produto abaixo
registra o que se pode sustentar para o caminho indicado. “Estudo” identifica
a leitura local realizada aqui, independente da disponibilidade no produto.
“Em aberto” exige teste do caminho ou oráculo semântico. As classes omitidas
não tiveram migração demonstrada. Nenhuma destas medições concede nível 6. O [catálogo por classe](perfis.tsv)
usa P para produto comprovado, E para estudo offline, travessão para classe
sem evidência no conjunto e interrogação para papel indeterminado.

| Fonte e natureza | Anatomia verificada e risco decisivo | Fronteira do produto e próximo oráculo |
|---|---|---|
| Koha SQL/gzip, dump misto | 149.654.596 bytes expandidos, 297 tabelas. 18.919 `biblio`, 18.919 `biblio_metadata`, 30.034 `items`, 1 `borrowers`, **1 `auth_header`**, 10 `old_issues`, 0 `issues`. Contagem igual entre tabelas ainda requer junção por chave. | Universal reconhece e recusa backup completo, nível 1 para esta entrada. SQL **parcial** bibliográfico tem testes no store. CLI dedicada migra leitores/circulação Koha com outro contrato. Conciliar IDs, MARC embutido e itens externos, comparar históricos e autoridade com oráculo local. |
| MRK Koha / Biblivre / Pergamum, exportações | Cada qual tem dez bibliográficos. Koha tem 18 campos 952, Biblivre tem 18 campos 949. Pergamum não apresenta nenhum deles. Todos têm dez 001 e sete 035. | Biblivre acervo/itens 5 na amostra, inclusive reapresentação e recusa de tombo ambíguo. Koha 4 ou superior nos casos cobertos de matching. No Pergamum exigir zero itens inventados. Não transferir o sentido de uma tag local entre origens. |
| Relatório `MARCXML-sample.sql` | Cada uma de 30 linhas tem um XML de registro, zero 952 interno e sufixo com sete separadores verticais. | Estudo 3 para o bloco XML. Criar leitor de envelope que conserve prefixo, XML e sufixo. Validar significado/identidade física do sufixo antes de somar itens. A extensão não autoriza tratá-lo como dump. |
| CSV fictício de leitores | 100 linhas, 84 colunas, 100 cartões distintos, 100 IDs distintos, quatro unidades, cinco categorias, um `flags=1`. | Transporte tabular existente. Leitores no importador universal em aberto. Oráculo de 100 destinos ou pendências explícitas, zero contas de equipe. Resolver unidade e categoria na prévia. |
| XLSX sem cabeçalho | 56 linhas, A–L, 672 células. A tem 53 e J tem 56 valores distintos. L usa strings. | Leitura de planilha existe. Agrupar usando A ou J muda a cardinalidade. Testar incluir a primeira linha como dado, mostrar os dois resultados, distinguir texto, formato de exibição e números já privados dos zeros. |
| `BKP_BIBLIOTECA (2).backup`, pg_dump 10.20 | COPY textual. 11.561 MFNs distintos em acervo, 18 registros locais e 72 títulos nulos/vazios. Das cinco ocorrências, três têm código de livro vazio/nulo. | Estudo 3. Fase 1a mede as duas tabelas. Futura projeção deve reter MFN com procedência, distinguir autoria pessoal/coletiva e deixar título e movimento incompletos para revisão. |
| Biblioteca Fácil `.bkp`, legado binário | 30 nomes distintos DAT/IDX e 33 fluxos zlib com final verificável sob limite de 4 MiB por fluxo. As duas quantidades medem coisas diferentes. | Estudo 1 com inventário parcial de rótulos. Faltam framing, layout, versão e relatórios do sistema para chaves, encoding, leitores, eventos e itens. Pedir cópia demonstrativa autorizada com totais e vínculos ou exportação intermediária. |
| FOLIO Cornell | TSV sem cabeçalho, UUID no envelope e objeto JSON. 62 Instâncias, 107 holdings e 536 itens. 107/107 holdings apontam para Instância do conjunto, 536/536 itens para holding. MARC separado tem 62 bibliográficos e 107 registros de holdings. | Estudo 3. Preservar hierarquia e campos repetíveis. Comparar identificadores e vínculos antes de combinar com o MARC. Estado do item não documenta um empréstimo. |
| FOLIO 5kItems | 47.745 Instâncias e 49.580 holdings, todos os pais encontrados. Nenhum arquivo de itens neste conjunto. | Estudo 3. Testar catálogo com holdings e sem contagem física inferida. Manter namespace separado de Cornell. |
| SLiMS | DDL/seed em `senayan.sql`, dados em `sampledata.sql`. 15 bibliográficos, 10 itens, 24 linhas de autoria, 29 atribuições de tópicos, 24 autores e 17 tópicos. | Estudo 3. Os papéis das tabelas de ligação explicam N:N. Verificar compatibilidade de colunas com o DDL. Autor nomeado numa lista e autoridade controlada exigem evidência distinta. |
| OpenBiblio | Quatro fragmentos. 22 registros, 93 cópias, 604 campos e 1.299 subcampos. Todas as cópias e campos têm pai por bibid, todos os subcampos têm pai por (bibid, fieldid). | Estudo 3. Reconstituir por sequência, tag e ocorrência, inclusive 001 com zeros. Sem amostra elegível de leitores ou histórico. |
| SQL de instalação Biblivre | Os dois `biblivre4.sql` são idênticos. A variante pg17 tem outro digest. Os scripts incluem schemas, traduções, configuração e autocomplete. Tabelas centrais declaradas aparecem sem dados de acervo. | Estudo 2 para as classes declaradas. Separar schema `global` de `bib4template`. Preservar conjuntos independentes e contar a cópia idêntica só uma vez por seleção. |
| Biblivre PGDMP de laboratório | Arquivo 1.16.0. 61 entradas TABLE DATA. Em `single`, 10 bibliográficos distintos, 20 holdings distintos com zero pais faltantes, 10 `users`, zero `authorities_records` e zero `lendings`. | Estudo 3 via utilitário PostgreSQL. Especificar significado dos cadastros e comparar ISO/identificadores e vínculos com relatório independente. Qualificação do runtime Rust permanece em aberto. |
| PMB | `minimum.sql` é seed. 913 INSERTs de parâmetros observados. Uma linha em `users` requer verificar papel; o corpus não comprova leitores catalogáveis. | Estudo 2 para estrutura observada. Obter exportação demonstrativa com registros, exemplares e leitores identificados. |
| SQLite sintético compactado | SQL textual, 30 tabelas. 12 livros, 40 cópias, 1.000 movimentos, 30 `users`, 35 matrículas, 10 perfis de equipe. Inclui pagamentos, papéis e auditoria. | Estudo 3. Teste negativo para `users = leitores` e para excluir lookups necessários junto com tabelas administrativas. A extensão gzip também aparece em Koha. |
| Autoridades em texto etiquetado | 12 líderes 000, 12 controles 001, doze 035, 22 campos 670. Há nome pessoal/coletivo, título, tópicos, lugares, gênero, subdivisão e campo local 199. Variantes 4XX e remissivas 5XX têm papéis diferentes. | Estudo 3. Preservar indicadores, notas, origem do termo, local 199 e subcampos. O caminho MARCXML bibliográfico recusa o primeiro líder de autoridade. Essa recusa é coberta com XML sintético. |
| Evergreen, ZIP | 43 arquivos Concerto Enhanced, 26 Concerto, 25 MARC e dois auxiliares. Commit indicado no pacote `2187620a632961edfa2780585b538f70bb8360d0`. Enhanced contém 344 `biblio.record_entry`, 195 `authority.record_entry`, 245 `authority.bib_linking` nas formas contadas. O arquivo chamado `auth-subset100.mrc` tem 17 registros e `auth-1066.mrc` tem 1.066 líderes z. O XML de holdings tem sete registros. | Inventário ZIP no produto, estudo 2–3 por membro. Concerto inclui INSERTs geradores com cardinalidade desconhecida. Não somar variantes, scripts de instalação, MARC e SQL como se fossem entidades de uma única biblioteca. A contagem de `actor.usr` inclui papéis a confirmar. |
| ABCD/ISIS, DSpace-CRIS, zstd de uma biblioteca | Ausentes neste corpus. A presença de zstd na função de backup operacional do Elzevir não qualifica importação bibliográfica. | Pedir exportação demonstrativa e oráculo de uma versão identificada. Para ISIS exigir MST/XRF e repetições, para DSpace distinguir metadados, autoridades, contas e objetos digitais. |

O manifesto registra somente estatísticas estruturais e digests. A pasta
original inclui tabelas de autenticação, cadastro e circulação. Direitos
de redistribuição e caráter fictício de todos os registros desses dumps não
estão comprovados. A análise ficou local, com resultados agregados. A
planilha de leitores identificada como fictícia e as duas fixtures autorais
têm finalidade de teste. Manter os originais nas condições de acesso do
mantenedor. O README de procedência de Evergreen distingue inclusive as
origens de seus arquivos MARC. Nenhuma informação pessoal foi necessária
para calcular as métricas.

## Código, lacunas e rastreabilidade

| Requisito | Caminho efetivo e limite observado | Prova existente ou próxima prova delimitada |
|---|---|---|
| Entrada, limites, neutralidade | `elzevir-app/src/import_detection` examina até 8 KiB. `import/containers` limita membros a 1.000, expansão a 512 MiB, razão a 100 e profundidade a 2. A API de contêiner recebe um slice e seleciona um membro. O limite do arquivo preparado é 512 MiB, com 100.000 registros. | `elzevir-app/tests/import_{detection,containers,table}.rs` e alvos de fuzz. Próxima prova COPY: terminador ausente, nulo versus literal, escapes, cancelamento, limites cumulativos, sem escrita. Dataset FOLIO dividido e ZIP com classes independentes em 1b. |
| Bibliográfico e itens | `import/koha_sql/{lexer,parser,assemble}`, `import/biblivre.rs`, `marc_mapping/items.rs` e `import/sheet.rs` produzem entrada para o catálogo. ISO original e projeção UNIMARC ficam na borda. `marc_mapping/export` é sensível a alterações catalográficas. | `elzevir-store/tests/{import_digest,import_preview,unimarc}.rs`; `elzevir/tests/import_batch.rs::biblivre_profile_reviews_commits_and_replays_the_eighteen_holdings`. Conservar digest, visibilidade de notas, bytes, 245, origens, titularidade e reimportação. |
| Identidade e confirmação | `elzevir-app/src/matching.rs` separa ausência, único, ambiguidade e conflito. `elzevir-store/src/matching.rs` consulta `import_identities` com origem, tipo, sistema e valor. `HoldingScope` inclui biblioteca, versão e empréstimo. O recibo do lote privado não equivale a um checkpoint multi-entidade. | `import_preview.rs` cobre rollback no último item e preservação local. `import_batch.rs` cobre mudança entre prévia e confirmação. Acrescentar reinício após commit antes do recibo e conflito cruzado entre origens na fase 3. |
| Autoridades | `elzevir-app/src/authorities.rs` e `elzevir-store/src/authorities.rs` oferecem atualização, vínculo por papel e alcance. A ligação assistida da spec v0 admite achar/criar por classe e nome, inadequado para consolidar duas autoridades desconhecidas. A borda `elzevir-marc/src/marcxml_reader/state.rs` interrompe bibliográficos ao ler líder de autoridade. | Novo teste `standalone_authorities_never_create_bibliographic_records` comprova recusa tipada no primeiro registro e digest estável. Fase 2 precisa de leitura por classe, equivalência MARC21/UNIMARC, registro independente, variantes, 5XX recíprocos, homônimos e reindexação da Obra ligada. |
| Leitores e circulação | `elzevir-app/src/koha_migration/{dump,accounts,circulation,history}.rs` e store dedicado tratam migração Koha. `koha_keys` usa (kind, koha_key) no contexto dessa operação. Reaproveitar validação de cadastro, restrições e regras de privacidade. O contrato de migração dedicada prevê senha do OPAC e tem pressupostos diferentes do importador universal. | `elzevir-store/tests/koha_migration.rs`, specs de `verify-koha`, circulação e LGPD. Exigir duas instalações com número repetido, leitor isolado, seleção mista, órfão, histórico consentido e zero privilégios. |
| Revisão, indexação e acesso | `import/{batches,preview,edits}.rs`, `elzevir/src/{import_service,import_batches,import_batch_cli}.rs` coordenam revisão e confirmação. Template Staff e CLI têm a mesma preparação. Comandos existentes deixam efeitos de reindexação na escritora. As permissões de catalogação e leitores têm fronteiras diferentes. | `elzevir/tests/{staff_import,import_batch}.rs` em servidor real, testes de OPAC/RDF e busca. Acrescentar matriz de acesso por classe, nota restrita e ausência de PII em diagnósticos, índice público, exportação e receitas. |

O doc 04 e o ADR 0003 definem BIBFRAME relacional e preservação do original.
O doc 05 reserva escritas a comandos tipados por instituição. A visão
histórica menciona categorias que o ADR 0003 substituiu. A proposta segue o
ADR vigente. A projeção tabular nativa continua usando o modelo bibliográfico,
sem fabricar um MARC intermediário. [Decisões](../../arquitetura/04-decisoes.md) ·
[Contratos da escritora](../../arquitetura/05-definicoes-de-implementacao.md) ·
[ADR BIBFRAME](../../adr/0003-bibframe-completo.md)

## Experimento de relação e alternativas de arquitetura

O [verificador local](verificar.py) usa apenas a biblioteca padrão de Python
para conferir digests e oráculos de desenvolvimento. O executável e o caminho
de importação continuam em Rust. Para reproduzir no checkout desta revisão:

```sh
python3 docs/evidencias/importacao-v4/verificar.py
cargo test --locked -p elzevir-store --test import_marcxml
cargo test --locked -p elzevir --test staff_import
```

O primeiro comando lê os digests dos 32 arquivos e de cada membro Evergreen,
confere a contagem Koha por uma segunda leitura independente do contador SQL
geral, percorre o COPY 10.20, confere os pais em OpenBiblio e compara as fixtures autorais com chaves e
relações escritas à mão. Com um cliente PostgreSQL 17 disponível, passar
`--pg-restore /caminho/para/pg_restore` ao verificador reproduz a contagem
das cinco tabelas do PGDMP e verifica o pai de cada holding. No experimento, a saída por linha é inclusão,
proposta após restrições e verdade esperada. Cada linha é **uma hipótese
sobre uma relação completa**, independentemente do número de registros.

| Método | Propostas corretas | Propostas falsas | Relações corretas omitidas | Abstenções corretas |
|---|---:|---:|---:|---:|
| Inclusão dos valores filhos nos valores pais | 4 | 3 | 0 | 1 |
| Inclusão + origem comum + pai único + papel corroborado | 3 | 0 | 1 | 4 |

Os casos positivos usam os vínculos de Cornell e a FK da fixture. Pares
de números locais iguais entre sistemas, IDs de operadores coincidentes e
chave-pai duplicada explicam as três propostas falsas. Um leitor órfão afeta
um movimento na fixture. A relação verdadeira omitida é o mesmo vínculo do
FOLIO com a evidência de papel retirada. O papel corroborado foi fornecido
pelo ensaio (FK ou semântica conhecida da fonte). O ensaio mede o efeito de
exigir essa evidência, e não a capacidade de descobri-la automaticamente.
O executor futuro deve mostrar também as linhas resolvíveis e o órfão, para
que a equipe possa agir por unidade.

Nesta máquina (Linux x86-64, EPYC 9V74, cgroup 4 CPUs e 32 GiB), uma execução
com cache aquecido do verificador incluindo os digests levou 0,82 s e 34.468
KiB de RSS máximo do processo filho, medida com `perf_counter` e
`getrusage(RUSAGE_CHILDREN)`. Estes números descrevem a leitura offline.
Tempo de confirmação, reindexação, concorrência e retomada do motor multi-origem
ficam por medir. As fases 1 e 6 devem separar leitura/staging/confirmação,
pico de RSS, disco, fila do writer, p95 da UI e efeitos de cancelamento em
1 mil, 100 mil e 1 milhão de registros, com repetição e cache declarados.
Usar o benchmark existente do store e a [spec de desempenho](../../specs/migracao/performance.md)
para a parte já implementada.

| Alternativa | Custo, explicabilidade e risco | Decisão |
|---|---|---|
| Ampliar cada importador dedicado com leitores e autoridades próprios | Baixo custo no primeiro perfil. Duplica confirmação, exclusão de privilégios e tratamento de órfãos. A troca dedicada já pressupõe uma instalação Koha. | Preservar os caminhos existentes e extrair operações comuns aos poucos. |
| Achatar todas as fontes em uma tabela | Barato para CSV. Perde ocorrência, ordem, vínculos N:N e distinção entre holding e peça. A planilha sem cabeçalho demonstra que um único eixo de agrupamento é insuficiente. | Oferecer grade como apresentação. Conservar endereçamento de registro, caminho e ocorrência sob a grade. |
| Grafo genérico persistente e solver global | Expressa relações concorrentes. Exige lifecycle, índices extras, explicação das escolhas e cópia persistente de PII. Não há evidência de que compense no primeiro leitor COPY. | Adiar. Relações candidatas com chave composta e contadores resolvem o primeiro ensaio sem infraestrutura nova. |
| Receita tipada com releitura do original e índice de chaves só onde necessário | Reusa revisão do lote e writer. Uma passagem caracteriza, outra executa o plano aprovado. Índice exato por chave tem custo de memória proporcional às chaves distintas; uma junção grande pode exigir armazenamento privado com expiração. | Escolhida para o primeiro incremento. Manter exemplos limitados por estrutura e por anomalia, medir cardinalidade e paginar. Qualquer aproximação serve para propor candidatos, toda confirmação exige checagem exata. |

**Mudança em relação ao desenho inicial.** Separar formalmente “há cobertura
de chave” de “há evidência do papel”. A interface pode perguntar uma vez pelo
papel e pela instituição para toda a relação compatível e ainda mostrar
exceções por linha. Essa economia de perguntas é uma hipótese a medir com
catalogadores. O plano mantém três projeções, seleção combinada, evidência,
prévia, preservação e um único writer. Adia grafo persistente, treino de pesos,
framework externo, novas dependências e IA até haver ganho mensurável.

## Comparação com referências

- **Cerulean Next.** O README público na revisão `246437a2` descreve treze
  etapas, snapshots, comparação, qualidade, sugestões de mapeamento e
  reconciliação. Declara licença proprietária da ByWater Solutions. A análise
  se limitou às funcionalidades declaradas no README. Aproveitar a pergunta ao
  operador sobre o que mudou entre revisões. Operações de desligar daemons e
  ajustar o banco do SIGB não cabem na prévia do Elzevir. [README](https://github.com/brendan1226/Cerulean-Next/blob/246437a2dcb0a513ef024de3f6a17370dd22dd09/README.md)
- **FOLIO Migration Tools.** A descrição da distribuição 1.14.0 documenta
  transformação, carga e relatórios para correção. Aproveitar separação entre
  configuração de mapeamentos e tarefa de carga. As chaves e a hierarquia de
  Cornell foram conferidas localmente. O Elzevir deverá distinguir pendência
  resolvível de falha da unidade. [Distribuição 1.14.0](https://pypi.org/project/folio-migration-tools/1.14.0/)
- **MarcEdit.** O material da Library Carpentry mostra profiling de campos,
  operações em lote e tarefas ordenadas reutilizáveis. Uma sequência que se
  reaplica pode duplicar uma adição de campo. Preservar original imutável,
  gerar diff a partir dele e separar editar receita de confirmar dados. A
  página de produto não estava acessível nesta sessão. [Tarefas e automação](https://github.com/LibraryCarpentry/lc-marcedit/blob/9df4a60266a792f3a7077bb18a4d45ff11d0e606/episodes/08-tasks-and-automation.md)
- **OpenRefine.** Facetas permitem delimitar problemas e reconciliação requer
  revisão humana de candidatos. Usar facetas de conflitos, valores ausentes
  e referências órfãs. A aprovação de agrupamento textual não autoriza fusão
  permanente de duas autoridades. [Manual de reconciliação](https://github.com/OpenRefine/openrefine.org/blob/9cbe4f396b95ef6037676815c11a2a717d56eee4/docs/manual/reconciling.md)
- **Library of Congress.** O conversor documenta entrada MARCXML, RDF/XML de
  saída e comparação com descrições BIBFRAME/MADS. Serve como referência
  externa para casos catalográficos pontuais. Guardar os bytes e o contexto
  local continua necessário para round-trip. Evitar recuperação de termos por
  rede durante a confirmação. [marc2bibframe2](https://github.com/lcnetdev/marc2bibframe2/blob/b099392b1bbe5caf34eacf522c4244a114216e4d/README.md)

## Auditoria da interface de equipe

No Chromium de sistema, com instituição descartável e conta sintética de
catalogação, o percurso login → Acervo/importar → upload do MRK Biblivre →
prever abriu a revisão 1 pronta para confirmação. O DOM exibiu `import-preview`
e `import-rules`. A captura local foi comparada com o PNG do mantenedor.
Os testes HTTP de Staff também exercitaram os envios de regras e a
confirmação de 949. Os dois procedimentos cobrem interação e persistência em
profundidades diferentes.

| Recurso na referência visual | Estado na interface de hoje | Mudança verificável |
|---|---|---|
| Fonte, formato, amostras e navegação | **Funcional** para upload, prévia, contagens e páginas. Estado e revisão aparecem no topo. | Incluir origem, membro, classes com permissões próprias e contagem por seleção. |
| Grade redimensionável e seleção de ocorrência | **Parcial.** Existem mapeamento de colunas, exemplos e edição. A tela MARC renderizada abre no formulário vertical de regras. | Manter estrutura repetível sob a grade, navegação por teclado, seleção explícita de campo/ocorrência e legenda para nulo/vazio. |
| Editar e restaurar receita | **Funcional** para adicionar, remover e aplicar regras de campo/subcampo por registro ou lote, alterar mapeamento de planilha e comparar prévia. **Parcial** para reuso entre origens. | Vincular receita redigida à versão e fingerprint estrutural, permitir reverter e comparar resultados. O original permanece inspecionável conforme permissão. |
| Destino pesquisável e ajuda contextual | **Parcial.** Há lista de destinos e formulários de cadastro, sem painel de evidências de vínculo ou campos normativos do mockup. | Mostrar papel, norma, alternativas, confiança fundamentada e impacto, em português e inglês. |
| Barra de ações, leitor e autoridade | **Funcional** para separar prévia, confirmação e cancelamento; controles de seleção das três classes estão **apenas na referência**. | Expressar seleção múltipla, erros por unidade e progresso com cancelamento. Testar foco após troca, leitor de tela e paridade com a CLI. |
| Correção inteligente e associação de exemplares | **Parcial.** Existe perfil 949 com confirmação de cadastros e tombo. Não há prova de correção ortográfica/associação genérica segura. | Toda sugestão precisa de diff, revisão e recusa possível. Exclusões de dados pessoais continuam ativas mesmo com IA desligada. |

Referências do fluxo: [template Staff](../../../crates/elzevir-web-templates/templates/staff_import.html),
[teste no servidor](../../../crates/elzevir/tests/staff_import.rs) e
[spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md).

## Validação independente e bloqueios

1. **Desenvolvimento congelado por digest.** Todo `samples-import-tool`,
   as fixtures autorais e os oito casos estudados pertencem a desenvolvimento.
   Os subgrupos do mesmo ZIP, variantes SQL ou linhas de uma mesma tabela
   não constituem famílias independentes.
2. **Validação.** Preparar variantes rotuladas separadamente por família.
   Traduzir e permutar colunas, separar matrícula de pessoa, mover holding
   para tabela ponte, introduzir chaves compostas, falsos cognatos, homônimos,
   ausência de classe, dados sujos e colisões entre sistemas. Validar tanto
   acerto quanto abstenção. Comparar o mesmo plano com perfil, sem perfil e
   com receita reaproveitada. IA só entra como quarto braço quando existir.
3. **Holdout.** Um mantenedor ou avaliador separado guarda uma família de
   esquema desconhecida e o oráculo assinado com chaves/relações. Fixar
   commit do motor, orçamento e regras de aceite antes da revelação. Se não
   houver fonte independente autorizada, outro avaliador gera esquema e
   valores sintéticos e lacra seed e gabarito antes da rodada. Depois de
   corrigir código usando esse caso, ele passa para regressão. O corpus
   disponível ainda não permite alegar aprovação desse teste.
4. **Aceite.** Para relações de risco, zero fusões, substituições, vínculos
   físicos ou privilégios incorretos nos casos rotulados. Exigir que todas as
   unidades e relações estejam contabilizadas; uma família inédita deve ir
   de inventário a gravação revisada com a mesma aplicação e sem patch por
   origem. Medir precisão e cobertura de classe, campo e relação, abstenção
   correta, órfãos, decisões manuais, tempo e RSS por classe. Calibrar metas
   adicionais com validação rotulada antes de abrir o holdout. As 3/3
   sugestões corretas do ensaio local não estimam precisão fora do ensaio.

Para bibliotecas com dados reais, a verificação da troca Koha exige oráculo
sob controle do mantenedor e relatórios agregados. Antes de liberar binários
exigir decodificador seguro e relatório do sistema por versão. Antes de
liberar seleção mista exigir as sete combinações e a matriz de vínculos da
spec contra SQLite WAL. Escala, acessibilidade completa, holdout e migração
operacional permanecem sem qualificação.

## Baseline e verificação local

| Verificação | Resultado |
|---|---|
| `cargo fmt --all --check` | Passou. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passou. Cargo avisou sobre incompatibilidade futura de `proc-macro-error2 2.0.1`. |
| `cargo xtask ci` | Passou. Camadas, tamanho, codegen, desvios e preferências verificados. |
| `cargo test --workspace --all-targets --locked --no-fail-fast` | Alternativa local a nextest, ausente no ambiente. Antes: 1.385 aprovados, dois ignorados. Depois do teste de proteção: 1.386 aprovados, zero falhas, os mesmos dois ignorados. |
| `cargo test --doc --workspace --locked` | Um doctest aprovado nas duas execuções. |
| `cargo test --locked -p elzevir --bin elzevir -- --ignored` | Os dois testes ignorados de integração local com /usr/bin/rclone passaram separadamente. |
| `cargo deny check` | Passou com cargo-deny 0.22.2 e configuração do projeto. 45 avisos de versões duplicadas e 11 de exceções a advisories não encontrados. A versão 0.18.5 não interpretava CVSS 4.0 da base RustSec atual. |
| Verificador de evidências e fluxo Chromium | Oito casos de vínculo, contagens/identidades verificadas e prévia Staff renderizada. O teste Rust adicional cobre recusa e integridade do catálogo. |

O banco operacional, schema, dependências de runtime e implementação de
produção conservam a revisão-base. Os testes locais não executaram piloto com
dados reais, benchmark de escrita a 1 milhão ou CI remota. O próximo ponto
de aprovação é o incremento 1a no doc 03.
