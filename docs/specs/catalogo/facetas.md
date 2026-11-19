# Facetas e filtros da listagem

> Status: tarefa 2.10, partes a (índice e aplicação), b (faixa na listagem) e c (listas de trabalho). Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 2, linha 2.10), [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §20 item 8, [busca](busca.md) e [listagem](listagem.md).

## 1. Comportamento

### Facetas

1. Oito facetas, cada uma com um nome estável que vai na URL:

   | Faceta | Nome | Valor no índice |
   |---|---|---|
   | Unidade | `branch` | o número da unidade de origem de cada exemplar da Instância, sem repetição |
   | Conteúdo | `content` | o nome do conteúdo (`bf:content`) que a descrição dá |
   | Suporte | `carrier` | o nome do suporte (`bf:carrier`) |
   | Gênero | `genre` | o rótulo de cada `bf:genreForm` da Obra (o da LCGFT para um IRI, item 12 do BIBFRAME completo) |
   | Público | `audience` | o público-alvo escrito (`521`), inteiro |
   | Idioma | `language` | o código de cada idioma da Obra |
   | Assunto | `subject` | o cabeçalho de cada assunto da Obra, inteiro |
   | Ano | `year` | os quatro primeiros algarismos seguidos da data de publicação, de `1000` a `2999` |

   Sem faceta de tipo de exemplar, que não existe no Elzevir (Paulo, 2026-11-15).
2. **Cada faceta conta famílias, não registros.** Família é a Matriz (Hub) com suas Obras, ou a Obra fora de Matriz, a mesma da listagem. Uma família com três Instâncias em português conta 1 em `language=por`. O total de famílias que casam também vem junto.
3. Cada faceta devolve até 20 valores, do que mais conta para o que menos conta; empate pela ordem alfabética do valor. Valor que é filtro ativo vem sempre, mesmo fora dos 20.

### Filtros

4. Um filtro é uma faceta e um valor, comparado inteiro. Filtros de facetas diferentes se somam (E). Dois valores da mesma faceta também se somam: `language=por` e `language=eng` pedem famílias com uma Instância nos dois idiomas. Valem até 16 filtros por pedido; os que passam disso são ignorados, e valor com mais de 200 caracteres também.
5. Busca com texto e filtros: o texto acha como antes ([busca](busca.md)), e os filtros restringem. Sem texto e com filtros, a listagem vem do índice: todas as Instâncias que passam nos filtros, agrupadas em família como numa busca. Sem texto e sem filtros, a listagem segue a ordem alfabética do banco ([listagem](listagem.md)).
6. O OPAC não usa filtros nesta parte.

### Faixa na listagem (parte b)

7. A URL da listagem leva cada filtro como `f=faceta:valor` (`/equipe/acervo?q=&f=language%3Apor`), repetido, ao lado de `q`, `after` e `from`. Parâmetro `f` sem `:`, faceta desconhecida ou valor fora dos limites do item 4 é ignorado.
8. Logo abaixo da busca fica uma faixa, sem barra lateral (interface §20, item 8), com:
   - o número de famílias que casam;
   - um menu por faceta que tem valores (`<details>`, sem script), cada valor com seu nome e a contagem de famílias. Clicar acrescenta o filtro; o valor que já é filtro aparece em destaque, sem link. Com 16 filtros, nenhum valor acrescenta mais;
   - cada filtro ativo como chip, que tira só ele ao ser clicado, e "Limpar filtros".
9. O nome de cada valor: unidade pelo nome do cadastro, conteúdo e suporte pelo rótulo da interface, idioma pelo nome da LC; gênero, público, assunto e ano como estão no índice.
10. O formulário de busca leva os filtros em campos ocultos: uma busca nova mantém os filtros. Com filtros, um código digitado é só texto (o bipe do item 8 de [bipe](bipe.md) vale sem filtros).
11. A faixa aparece na listagem, na busca e na busca filtrada; não aparece na página de um código bipado (é a família sozinha) nem nas linhas da página seguinte. A página seguinte leva os mesmos filtros.
12. As facetas do acervo inteiro (sem texto e sem filtros), que toda página de navegação mostra, ficam guardadas por geração do leitor do índice e são recalculadas depois de cada commit.

### Listas de trabalho (parte c)

13. A faixa começa com as listas de trabalho como botões segmentados (interface §20, item 8): **Todo o acervo**, **Meus recentes** e **Importados hoje**. A lista escolhida aparece marcada (`aria-current`); fora de uma lista, a marcada é "Todo o acervo".
14. Cada lista é uma visão da listagem, com endereço próprio: `/equipe/acervo?list=recent` e `?list=imported-today`. Nome de lista desconhecido é ignorado. A lista não se soma a texto nem a filtros: com `q` ou `f`, vale a busca; buscar pelo formulário sai da lista, e os botões levam à lista sem texto nem filtros.
15. **Meus recentes**: as famílias em que a pessoa logada mexeu (criou, editou, ligou autoridade, salvou exemplares), lidas do `audit_log` pelas entidades de cada comando (a mesma origem do [histórico](historico.md)): Obra pela sua família, Instância pela família da sua Obra. Ordem: a família tocada por último primeiro. Vale o que os 2.000 comandos mais recentes da pessoa tocaram.
16. **Importados hoje**: as famílias das Instâncias vindas de arquivo (`catalog.import_record`, e `catalog.add_record` com registro de origem) desde a meia-noite de hoje no fuso da biblioteca, a mais recente primeiro, entre os 2.000 comandos de importação mais recentes do dia. O fuso é `[library] time_zone` no `elzevir.toml` (nome IANA, como `America/Sao_Paulo`); sem ele, o fuso do sistema.
17. Família que deixou de existir sai da lista. A página tem 30 famílias, como a listagem; a seguinte vem pelo mesmo carregamento (`from=`), sem faixa. Numa lista, a faixa não tem contagem nem menus de facetas: só os botões.
18. Ficam para depois, porque dependem de funções que ainda não existem: **Em quarentena** (a quarentena chega com a mesclagem da Onda 2, interface §7) e **Incompletos para o perfil** (os perfis por categoria saíram com o BIBFRAME completo, [ADR 0003](../../adr/0003-bibframe-completo.md); falta decidir o que "incompleto" quer dizer sem perfil).

## 2. Origem no Koha

O Koha mostra facetas na busca do OPAC e da equipe (`C4::Search::getRecords` com `Koha::SearchEngine::Elasticsearch::QueryBuilder::build_query`, que pede agregações; as facetas configuráveis em `admin/searchengine/elasticsearch/mappings.yaml` com `facet: 1`: autor, assunto, assunto geográfico, série, tipo de item, unidade de origem, unidade atual, localização, idioma). Conta registros bibliográficos, e um filtro novo restringe a busca (`limit=`). O Elzevir conta famílias, não tem tipo de item e acrescenta conteúdo, suporte, gênero, público e ano.

## 3. Paridade

`livre` (busca e interface). Prova:

- `elzevir-search` (unidade): documentos com Hub e sem Hub contam por família; filtro de uma e de duas facetas; dois valores da mesma faceta; valor ativo fora dos 20 aparece; ano tirado da data; sem texto e com filtros.
- `elzevir-app::listing`: busca com filtros vira página de famílias; filtros demais e valores longos são cortados.
- `crates/elzevir/tests/search.rs`: sobre os 200 registros da base de testes, as contagens batem com uma contagem feita direto no banco.
- `elzevir-web` (unidade): filtros da URL e endereços de acrescentar e tirar; nomes dos valores. `crates/elzevir-web/tests/staff_catalog.rs`: faixa na navegação, filtro sozinho vira busca, chip, campo oculto, valor ativo sem link, filtro desconhecido ignorado, linhas da página seguinte sem faixa.
- `elzevir-search`: as contagens guardadas do acervo inteiro mudam depois de um commit.
- `elzevir-store` (unidade): "meus recentes" agrupa por família, na ordem do último toque, só da pessoa; "importados hoje" pega importação de arquivo depois do instante dado e deixa de fora o que foi criado na tela. `elzevir-app::worklist`: páginas sem buraco nem repetição. `crates/elzevir-web/tests/staff_catalog.rs`: botões das listas, lista marcada, lista com a pessoa logada, página seguinte com `list=`.

Desempenho (critério da 2.10: p95 < 50 ms em 1 milhão de registros): `cargo bench -p elzevir --bench opac` mede a página de navegação e a de busca já com a faixa, e uma página filtrada (só `language:por`, e palavras com `language:por`), e a primeira a décima página de "Importados hoje". `ELZEVIR_BENCH_DIR` guarda o acervo montado e mede de novo sem importar.

Medida de 2026-11-19, numa máquina de 4 núcleos, 1 milhão de registros, 2.000 páginas cada, com o cache do sistema já quente:

| Página | p50 | p95 | p99 |
|---|---|---|---|
| Navegação | 12,6 ms | **18,4 ms** | 31,4 ms |
| Busca | 13,5 ms | **21,7 ms** | 115,0 ms |
| Filtrada | 30,8 ms | **48,0 ms** | 54,7 ms |
| "Importados hoje" | 15,0 ms | **19,8 ms** | 24,8 ms |

Antes desta medida a página filtrada dava p95 de 120 ms e "Importados hoje", 78 ms. Três mudanças trouxeram as duas para baixo do critério:

- **Segmentos de até 64 mil documentos.** As facetas de um filtro que casa com quase todo o acervo (985 mil famílias) contam cada documento. O executor espalha os segmentos do índice pelos núcleos, mas a política de junção do Tantivy fazia um segmento de 800 mil, trabalho de um núcleo só. A política do Elzevir é a mesma, cortando cada junção em 64 mil documentos: a contagem caiu de 90 ms para 43 ms, e a busca por palavras não ficou mais lenta. Um índice já montado só ganha segmentos menores quando é reconstruído.
- **Facetas contadas ao mesmo tempo que a página.** A contagem roda numa thread própria enquanto a busca e as famílias da página são lidas.
- **Índice parcial das importações** (`audit_imports`, migração 43). "Importados hoje" lia o JSON de cada um dos 2.000 comandos mais recentes para saber se tinha registro de origem: 50 ms. O índice guarda essa condição, e a consulta caiu para 0,6 ms.

A página filtrada fica a 2 ms do limite. Um índice frio, logo depois de ligar o servidor, é mais lento nos primeiros pedidos.
