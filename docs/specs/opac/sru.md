# Servidor SRU do catálogo público (v0)

> Status: v0, tarefa 4.6b. Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md), Fase 4, tarefa 4.6; [OAI-PMH](oai-pmh.md) (4.6a); [busca](../catalogo/busca.md); [ocultar no OPAC](../catalogo/ocultar-no-opac.md); [fontes de rascunho](../catalogo/fontes-de-rascunho.md) (o cliente SRU do Elzevir). Protocolo: SRU 1.1, 1.2 e 2.0 com CQL, [diagnósticos](https://www.loc.gov/standards/sru/diagnostics/diagnosticsList.html) `info:srw/diagnostic/1`. Critério da tarefa: **um cliente externo encontra e colhe os registros visíveis no OPAC, nada oculto**.

Um cliente SRU (outro Koha, o próprio Elzevir, um catálogo coletivo) busca o acervo público por CQL e recebe MARCXML ou Dublin Core. A busca é a do OPAC: o mesmo índice, sem as Instâncias ocultas.

## 1. Comportamento

### Configuração

1. A seção `[sru]` do `elzevir.toml` liga o servidor. Sem a seção, ou com `enabled = false`, `/sru` responde **404**.
2. Chaves:
   - `enabled` (padrão `false`);
   - `base_url`: o endereço público de `/sru`, `http://` ou `https://`, sem consulta nem fragmento, até 512 caracteres. Obrigatório quando ligado. O `explain` tira dele o host, a porta e o caminho (o "banco");
   - `title`: o nome do catálogo no `explain`, de 1 a 200 caracteres. Obrigatório quando ligado;
   - `default_records`: registros por resposta quando o cliente não pede (padrão **10**), de 1 a `max_records`;
   - `max_records`: o máximo por resposta (padrão **50**), de 1 a 1.000. Um pedido maior recebe esse máximo, sem erro.
   Chave desconhecida ou valor fora da regra impede o servidor de subir, com a mensagem do campo.

### Pedido

3. `GET /sru?…` e `POST /sru` com corpo `application/x-www-form-urlencoded` (até 8 KiB) aceitam os mesmos parâmetros. A resposta é `text/xml; charset=utf-8` e tem status **200** também quando traz um diagnóstico, como pede o SRU. A rota fica no OPAC, sem sessão.
4. Parâmetros aceitos: `operation`, `version`, `query`, `startRecord`, `maximumRecords`, `recordSchema`, `recordPacking`, `recordXMLEscaping`, `queryType`, `resultSetTTL`, `stylesheet`, `httpAccept`, `sortKeys` e os de extensão (`x-…`). Os três últimos e `resultSetTTL`, `stylesheet` são lidos e ignorados (sem conjuntos de resultado guardados nem folha de estilo). Outro parâmetro dá o diagnóstico **8** com o nome dele. Parâmetro repetido dá **6**. Valor com mais de 2.048 caracteres dá **6**.
5. **Versão**: `version` 1.1 ou 1.2 respondem no envelope do SRU 1.x (`http://www.loc.gov/zing/srw/`); 2.0 no do SRU 2.0 (`http://docs.oasis-open.org/ns/search-ws/sruResponse`). Sem `version`, vale 1.2. Outra versão dá **5**, com `1.2` nos detalhes, no envelope 1.2.
6. **Operação**: `searchRetrieve` ou `explain`. Sem `operation`, é `searchRetrieve` quando há `query` (o SRU 2.0 não tem o parâmetro) e `explain` quando não há. `scan` e qualquer outra dão **4**.
7. `searchRetrieve`:
   - `query` obrigatório (**7**, com `query` nos detalhes);
   - `queryType`, quando vem, só `cql` (**6**);
   - `startRecord` (padrão 1) e `maximumRecords` (padrão `default_records`) são inteiros ≥ 1 e ≥ 0 (**6** com o nome); `maximumRecords = 0` só conta;
   - `startRecord` além do total, com total maior que zero, dá **61**;
   - `recordSchema` (padrão `marcxml`): `marcxml`, `marc21`, `info:srw/schema/1/marcxml-v1.1`, `dc` ou `info:srw/schema/1/dc-v1.1`. Outro dá **66** com o nome;
   - `recordPacking` (1.x) ou `recordXMLEscaping` (2.0): `xml` (padrão) ou `string`, que manda o registro escapado como texto. Outro dá **71**. No 2.0, `recordPacking` (`packed`/`unpacked`) é aceito e ignorado.

### CQL

8. A consulta é CQL 1.2: cláusulas `índice relação termo` ou só `termo`, ligadas por `and`, `or` e `not` (sem diferença de maiúsculas, da esquerda para a direita) e agrupadas por parênteses. Termo entre aspas aceita `\"` e `\\`.
9. **Índices** (os nomes vêm do `pqf.properties` do Koha; sem prefixo, o conjunto é `dc`):

   | Índice CQL | Campo da busca |
   |---|---|
   | `cql.serverChoice`, `dc.any`, termo sem índice | tudo, como a caixa de busca do OPAC (palavras e códigos exatos) |
   | `dc.title`, `bath.keyTitle`, `bath.uniformTitle` | títulos da Instância e da Obra, e os outros títulos |
   | `dc.creator`, `dc.author`, `dc.editor`, `dc.pname`, `bath.name`, `bath.personalName`, `bath.corporateName`, `bath.conferenceName` | contribuidores |
   | `dc.subject`, `bath.topicalSubject`, `bath.geographicName`, `bath.genreForm` | assuntos |
   | `dc.publisher` | editora |
   | `dc.description`, `dc.note`, `bath.notes` | resumo e notas |
   | `dc.identifier`, `dc.isbn`, `dc.issn`, `bath.isbn`, `bath.issn` | identificadores (ISBN sem hífen) |
   | `dc.date` | ano de publicação |
   | `dc.language` | código do idioma |
   | `dc.barcode`, `koha.barcode` | código de barras |
   | `rec.id`, `dc.resourceIdentifier` | número da Instância |

   Conjunto conhecido (`cql`, `dc`, `bath`, `rec`, `koha`) com índice fora da tabela dá **16** com o nome; outro prefixo dá **15**. Os índices do Koha de exemplar por código local (tipo, unidade, localização) ficam de fora: o núcleo não tem códigos locais.
10. **Relações**:
    - campos de palavras (título, contribuidor, assunto, editora, notas): `=` e `all` pedem todas as palavras; `any`, qualquer uma; `adj`, `==` e `exact`, a frase. As palavras passam pela mesma normalização do OPAC (sem acento, sem maiúsculas, radical);
    - tudo (`serverChoice`): `=`, `all` e `adj` como a caixa do OPAC; `any`, qualquer palavra;
    - códigos (identificador, ano, idioma, código de barras, número): `=`, `==` e `exact`, valor inteiro.
    Outra relação dá **19** com a relação. Modificador (`=/stem`) dá **20**; modificador em booleano e `prox` dão **37**.
11. Termo vazio (`""`) dá **27**. `*` ou `?` sem `\` dá **28**. Parênteses sem par dão **13**. `sortBy` dá **80**. Atribuição de prefixo (`>dc="…"`) e o resto que a gramática não lê dão **10**.
12. **Limites**: até 64 cláusulas e 16 níveis de parênteses; mais que isso dá **10**.

### Resultado

13. A busca usa o escopo do OPAC: **Instância oculta não entra** no total nem nos registros. Ordem: relevância, como o OPAC.
14. Cada registro é a Instância como a [exportação MARC](../marc/escrita.md) a grava, **com os exemplares** (952), como o servidor Z39.50/SRU do Koha entrega. Em Dublin Core, o crosswalk é o do [OAI-PMH](oai-pmh.md), item 16, no elemento `srw_dc:dc` (`info:srw/schema/1/dc-schema`).
15. O índice é atualizado depois da gravação. Uma Instância que ficou oculta, sumiu ou não se grava em MARC entre a busca e a leitura sai como **diagnóstico substituto** no lugar do registro (`recordSchema` `info:srw/schema/1/diagnostics-v1.1`, diagnóstico **64**), sem nenhum dado dela.
16. `numberOfRecords` é o total; `nextRecordPosition` vem quando há mais registros depois da página. Cada registro leva `recordSchema` (o nome pedido, normalizado: `marcxml` ou `dc`), `recordPacking` (1.x) ou `recordXMLEscaping` (2.0), `recordData` e `recordPosition`.
17. Um diagnóstico traz `uri` (`info:srw/diagnostic/1/N`), `details` quando há, e `message` (a descrição da lista do SRU). Falha do banco ou do índice dá **1** ("General system error") sem detalhe, e o log registra a causa.

### Explain

18. `explain` devolve um registro ZeeRex (`http://explain.z3950.org/dtd/2.0/`): `serverInfo` (protocolo SRU, versão, host, porta e banco do `base_url`), `databaseInfo` (o `title`), `indexInfo` (os conjuntos e cada índice da tabela do item 9), `schemaInfo` (`marcxml` e `dc`) e `configInfo` (`default_records`, `max_records`).

### Registro

19. Cada pedido gera o evento `sru.request` com operação, versão, resultado (`answered` ou o número do diagnóstico), total e registros enviados. Registros que saíram como diagnóstico substituto geram `sru.record_unavailable` com os números das Instâncias. A consulta não vai para o log.

## 2. Origem no Koha

O Koha serve SRU de duas formas: o Zebra (`publicserver` do `koha-conf.xml`) e o `misc/z3950_responder.pl` (`Koha::Z3950Responder`), que atende Z39.50 e SRU na mesma porta pelo YAZ e traduz CQL pelo `etc/z3950/pqf.properties`. A busca do responder vai ao motor do OPAC (`Koha::SearchEngine::Search`) e os registros saem com os exemplares, opcionalmente com a situação de cada um (`--add-item-status`). Diferenças intencionais:

- **Ocultos**: o Koha não filtra `OpacHiddenItems` nem `opac_suppression` no servidor Z39.50/SRU; o Elzevir busca no escopo do OPAC e confere de novo na leitura (itens 13 e 15), como pede o critério da tarefa;
- **`=` em palavras** pede todas as palavras em qualquer ordem; no Koha, pelo `pqf.properties`, vira frase (`4=1`) para o Zebra. "Teodora Lemos" acha "Lemos, Teodora";
- **Índices**: só os da tabela do item 9, que têm campo no índice do Elzevir; os do Koha por código de exemplar ficam de fora;
- **Rota** `/sru` no servidor web, em vez de uma porta própria.

## 3. Paridade

`livre` (busca). Prova:

- `elzevir-app::sru` (unidade): gramática CQL, índices, relações e cada diagnóstico; parâmetros e versões;
- `crates/elzevir/tests/sru.rs`, sobre os 200 registros de exemplo:
  - `explain` com e sem `operation`;
  - busca por título, autor (palavras e frase), código de barras, ano, ISBN com hífen, número da Instância, `and`/`or`/`not` e parênteses, sobre o índice Tantivy;
  - páginas com `startRecord` e `nextRecordPosition`; `maximumRecords=0`;
  - MARCXML com 952 e Dublin Core; `recordPacking=string`;
  - SRU 2.0 no envelope OASIS;
  - Instância oculta fora do total e dos registros;
  - os diagnósticos 4, 5, 6, 7, 8, 10, 13, 16, 19, 27, 28, 61, 66 e 71;
  - **o cliente SRU do Elzevir** (`elzevir-sources`) buscando no servidor do Elzevir, em 1.2 e 2.0, e recebendo os registros;
  - 404 sem a seção.

## 4. Fora desta tarefa

- `scan` (lista de termos de um índice) e conjuntos de resultado guardados (`resultSetId`).
- Ordenação (`sortBy`, `sortKeys`).
- Situação do exemplar acrescentada ao 952 (`--add-item-status` do Koha).
- Busca de autoridades por SRU.
- Z39.50: tarefa 4.6c, que reaproveita a busca por campo desta.
