# Facetas e filtros da listagem

> Status: tarefa 2.10, parte a (índice e aplicação). Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 2, linha 2.10), [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §20 item 8, [busca](busca.md) e [listagem](listagem.md). A faixa horizontal com menus e chips é a parte b; as listas de trabalho salvas, a parte c.

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

## 2. Origem no Koha

O Koha mostra facetas na busca do OPAC e da equipe (`C4::Search::getRecords` com `Koha::SearchEngine::Elasticsearch::QueryBuilder::build_query`, que pede agregações; as facetas configuráveis em `admin/searchengine/elasticsearch/mappings.yaml` com `facet: 1`: autor, assunto, assunto geográfico, série, tipo de item, unidade de origem, unidade atual, localização, idioma). Conta registros bibliográficos, e um filtro novo restringe a busca (`limit=`). O Elzevir conta famílias, não tem tipo de item e acrescenta conteúdo, suporte, gênero, público e ano.

## 3. Paridade

`livre` (busca e interface). Prova:

- `elzevir-search` (unidade): documentos com Hub e sem Hub contam por família; filtro de uma e de duas facetas; dois valores da mesma faceta; valor ativo fora dos 20 aparece; ano tirado da data; sem texto e com filtros.
- `elzevir-app::listing`: busca com filtros vira página de famílias; filtros demais e valores longos são cortados.
- `crates/elzevir/tests/search.rs`: sobre os 200 registros da base de testes, as contagens batem com uma contagem feita direto no banco.

O critério de desempenho da 2.10 (p95 < 50 ms em 1 milhão de registros) é medido na parte b, que põe as facetas na página e no `cargo bench -p elzevir --bench opac`.
