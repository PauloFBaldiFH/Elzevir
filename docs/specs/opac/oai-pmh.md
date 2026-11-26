# Colheita OAI-PMH do catálogo público (v0)

> Status: v0, tarefa 4.6a. Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md), Fase 4, tarefa 4.6; [OPAC](busca-e-registro.md); [ocultar no OPAC](../catalogo/ocultar-no-opac.md); [escrita MARC](../marc/escrita.md); protocolo [OAI-PMH 2.0](https://www.openarchives.org/OAI/openarchivesprotocol.html). Critério da tarefa: **um cliente externo encontra e colhe os registros visíveis no OPAC, nada oculto**.

A tarefa 4.6 tem três partes, cada uma num PR:

- **4.6a, OAI-PMH** (esta spec): colheita dos registros do OPAC por `/oai`.
- **4.6b, servidor SRU**: busca do OPAC por SRU 1.2/2.0, com CQL, em spec própria.
- **4.6c, servidor Z39.50**: a mesma busca por Z39.50, sobre o BER da [tarefa 4.3](../catalogo/cliente-z3950.md), em spec própria.

Um registro OAI é uma **Instância**, como no Koha um registro é um `biblio`: é a unidade que a exportação MARC grava e que o OPAC mostra em `/record/{id}`.

## 1. Comportamento

### Configuração

1. A seção `[oai]` do `elzevir.toml` liga o repositório. Sem a seção, ou com `enabled = false`, `/oai` responde **404**, como o Koha com a preferência `OAI-PMH` desligada (o padrão).
2. Chaves:
   - `enabled` (padrão `false`);
   - `repository_identifier`: o prefixo dos identificadores (o `OAI-PMH:archiveID` do Koha). Obrigatório quando ligado, de 1 a 64 caracteres entre letras, dígitos, `.`, `-` e `_`;
   - `base_url`: o endereço público de `/oai` que o `Identify` informa, `http://` ou `https://`, sem consulta nem fragmento. Obrigatório quando ligado, porque o servidor atrás de um proxy não sabe o próprio endereço e o cabeçalho `Host` vem do cliente;
   - `repository_name`: o nome que o `Identify` informa (o `LibraryName` do Koha), de 1 a 200 caracteres. Obrigatório quando ligado;
   - `admin_email`: um ou mais endereços. Obrigatório quando ligado, porque o OAI-PMH exige pelo menos um;
   - `max_count`: registros por página, de 1 a 1.000 (padrão **50**, como `OAI-PMH:MaxCount`).
   Chave desconhecida ou valor fora da regra impede o servidor de subir, com a mensagem do campo.

### Pedido e resposta

3. `GET /oai?verb=…` e `POST /oai` com corpo `application/x-www-form-urlencoded` (até 8 KiB) aceitam os mesmos argumentos. A resposta é `text/xml; charset=utf-8`, com status 200 também quando traz um erro OAI, como pede o protocolo. A rota fica no OPAC, sem sessão.
4. A resposta segue o esquema do OAI-PMH 2.0: `responseDate` (agora, em UTC, `AAAA-MM-DDThh:mm:ssZ`), `request` com o `base_url` e os argumentos aceitos (sem eles quando há `badVerb` ou `badArgument`), e o elemento do verbo ou os `error`.
5. Validação, antes de qualquer leitura:
   - verbo ausente, repetido ou desconhecido: `badVerb`;
   - argumento repetido, desconhecido para o verbo, obrigatório que falta, ou valor com mais de 256 caracteres: `badArgument`;
   - `resumptionToken` junto de outro argumento além de `verb`: `badArgument`;
   - `from` ou `until` que não é `AAAA-MM-DD` nem `AAAA-MM-DDThh:mm:ssZ`, os dois com granularidades diferentes, ou `from` depois de `until`: `badArgument`;
   - `metadataPrefix` fora da lista do item 8: `cannotDisseminateFormat`.
   Vários erros saem juntos, como faz o `validate_request` do Koha.

### Identificadores, data e registros apagados

6. O identificador de um registro é `<repository_identifier>:<id da Instância>`, como o `<archiveID>:<biblionumber>` do Koha. Um identificador com outro prefixo, ou cujo resto não é um número inteiro positivo, dá `idDoesNotExist`.
7. A **data do registro** (`datestamp`) é a última vez em que a versão da Instância subiu (`instances.row_version`), em UTC e com segundos. É a mesma versão que o cache do OPAC usa, então tudo que muda o registro mostrado conta: edição da Instância e da Obra, renomeação de autoridade ligada, exemplar acrescentado, marca de oculta. A migração nova guarda essa data numa tabela própria, preenchida no mesmo comando da escritora; para as Instâncias que já existem, vale o momento da migração.
8. Uma Instância **oculta do OPAC** sai como **apagada**: só o cabeçalho, com `status="deleted"` e a data em que foi ocultada, nunca os metadados. Assim o colhedor que já tinha o registro o retira, e nenhum dado oculto sai pelo OAI. O Identify declara `deletedRecord` = `persistent`: a marca de apagado fica enquanto a Instância existir. Ao voltar a aparecer, o registro sai inteiro, com a data da volta. O Elzevir ainda não apaga Instâncias; quando apagar, a data fica na mesma tabela e o registro sai como apagado.

### Verbos

9. **Identify**: `repositoryName`, `baseURL`, `protocolVersion` 2.0, `adminEmail` (um por endereço), `earliestDatestamp` (a menor data do item 7, ou `0001-01-01T00:00:00Z` sem registros, como o Koha), `deletedRecord` `persistent` e `granularity` `YYYY-MM-DDThh:mm:ssZ`. Não leva `description`, como o Koha, nem `compression`.
10. **ListMetadataFormats**: `oai_dc`, `marc21` e `marcxml`, nessa ordem, com os esquemas e espaços de nome do Koha. Com `identifier`, a mesma lista para um registro que existe, ou `idDoesNotExist`.
11. **ListSets**: `noSetHierarchy`, porque o v0 não tem conjuntos. `set` em `ListIdentifiers` ou `ListRecords` também dá `noSetHierarchy`.
12. **GetRecord** (`identifier`, `metadataPrefix`): o registro no formato pedido; oculto, só o cabeçalho apagado; inexistente, `idDoesNotExist`.
13. **ListIdentifiers** e **ListRecords** (`metadataPrefix`, `from`, `until`, `set`, ou só `resumptionToken`): os registros com data entre `from` e `until`, os dois inclusive, **em ordem de id**, até `max_count` por resposta. `from` só com dia vale `T00:00:00Z`, e `until` só com dia vale `T23:59:59Z`, como o `ResumptionToken` do Koha. Nenhum registro: `noRecordsMatch`.
14. **Continuação**: quando sobram registros, a resposta termina com `<resumptionToken cursor="N">`, onde `N` é quantos registros as respostas anteriores já deram. O token tem a forma do Koha, `<prefixo>/<cursor>/<from>/<until>/<set>/<próximo id>`, e só carrega estado: a próxima página relê o banco a partir do próximo id, então um registro que muda no meio da colheita aparece com a data nova. Na última página de uma lista continuada, o token sai vazio (`<resumptionToken cursor="N"/>`), como pede o protocolo. Token que não se lê, com prefixo desconhecido, cursor ou id negativo, ou mais de 512 caracteres: `badResumptionToken`.

### Metadados

15. `marc21` e `marcxml` são o mesmo **MARCXML** de um registro (`<record xmlns="http://www.loc.gov/MARC21/slim">`), gerado pela [escrita MARC](../marc/escrita.md) da Instância, **sem os campos de exemplar** (`952`), como o Koha no modo simples (`include_items` desligado). O registro leva o `942 $n` da marca de oculta, como a exportação.
16. `oai_dc` é o **Dublin Core simples**, tirado desse MARC pelas regras do `MARC21slim2OAIDC.xsl` do Koha, nesta ordem:
    - `dc:title`: `245 $a $b $f $g $h $k`;
    - `dc:creator`: cada `100`, `110`, `111`, `700`, `710`, `711` e `720`, com os subcampos;
    - `dc:type`: `collection` quando o líder 07 é `c`, `manuscript` quando o líder 06 é `d`, `f`, `p` ou `t`, e o tipo pelo líder 06 (`text`, `cartographic`, `notated music`, `sound recording`, `still image`, `moving image`, `three dimensional object`, `software, multimedia`, `mixed material`), num só elemento, separados por espaço; depois um por `655`, com os subcampos;
    - `dc:publisher` (`260`/`264` `$a $b`) e `dc:date` (`$c`), primeiro os `260`, depois os `264`;
    - `dc:language`: `008` posições 35 a 37;
    - `dc:format`: cada `856 $q`;
    - `dc:description`: o primeiro `$a` de cada `520`, de cada `521` e dos outros `501` a `599`, menos `506`, `530`, `540` e `546` (o `500` fica de fora, como no XSL, que pede etiqueta maior que 500);
    - `dc:subject`: `600`, `610`, `611`, `630`, `650` e `653`, `$a $b $c $d $q`;
    - `dc:coverage`: `752 $a $b $c $d`;
    - `dc:relation`: `530 $a $b $c $d $u`, e `$o $t` dos `760` a `787` de ligação;
    - `dc:identifier`: o primeiro `$u` de cada `856`, e `URN:ISBN:` com o primeiro `$a` de cada `020`;
    - `dc:rights`: o primeiro `$a` de cada `506` e de cada `540`.
    Os subcampos se juntam com um espaço, e os campos se repetem na ordem do registro dentro de cada etiqueta. Elemento que ficaria vazio não sai.

### Limites, log e desempenho

17. Uma resposta tem no máximo `max_count` registros; um pedido nunca lê mais que `max_count + 1` linhas por página. A data de cada registro vem da mesma consulta da página, e o MARC de cada um é montado um a um, sem guardar a página inteira em memória além da resposta.
18. Falha do banco responde **503** com texto curto, como o OPAC; erro ao montar um registro (MARC inválido) sai como o registro apagado, com a falha no log, para não interromper a colheita.
19. Cada pedido registra o evento `oai.request` com o verbo, o formato, o resultado (`answered`, um código de erro OAI ou `unavailable`) e quantos registros saíram, sem o endereço do cliente.

## 2. Origem no Koha

- `opac/oai.pl` e `Koha::OAI::Server::Repository` (validação por `HTTP::OAI::Repository::validate_request`, formatos `oai_dc`, `marc21`, `marcxml`, `OAI-PMH:archiveID`, `OAI-PMH:MaxCount`, `OAI-PMH:DeletedRecord`).
- `Koha::OAI::Server::Identify`, `ListMetadataFormats`, `ListSets`, `GetRecord`, `ListBase` (`ListIdentifiers`, `ListRecords`), `ResumptionToken` e `DeletedRecord`.
- `Koha::OAI::Server::Repository::get_biblio_marcxml`: registro suprimido no OPAC ou oculto por `OpacHiddenItems` sai sem metadados, como apagado.
- `koha-tmpl/intranet-tmpl/prog/en/xslt/MARC21slim2OAIDC.xsl` e `subfieldSelect` do `MARC21slimUtils.xsl`.
- Diferenças intencionais:
  - configuração tipada no `elzevir.toml`, com `base_url` explícito em vez do endereço montado a partir do pedido;
  - sem conjuntos (`oai_sets`) e sem o arquivo de formatos extras (`OAI-PMH:ConfFile`) no v0;
  - o Dublin Core junta os subcampos e as partes do `dc:type` com espaço (o `xsl:value-of` do Koha cola o texto sem separador, ou com o espaço em branco do MARCXML indentado), não repete o `520` e o `521` em `dc:description`, não escreve elementos vazios e não põe o atributo `type="original"`, que o esquema do `oai_dc` não tem;
  - a data é a da versão do registro, e não a do `timestamp` da tabela `biblio_metadata`;
  - a última página de uma lista continuada traz o token vazio.

## 3. Paridade

`livre`: o OAI-PMH não está nas áreas de paridade exigida. O Koha é a referência dos formatos, do identificador e do token; o protocolo é a referência do resto.

Testes: snapshots `insta` de cada verbo e de cada erro, a colheita completa em páginas com um registro oculto no meio (cabeçalho apagado, nenhum metadado), `from`/`until` nos limites do dia e do segundo, token adulterado, e o Dublin Core dos registros de teste da importação MARC comparado com o resultado do XSL do Koha gravado como fixture.

## 4. Fora desta tarefa

- Conjuntos (`ListSets` com `oai_sets`), formatos extras por configuração e exemplares no MARCXML.
- SRU e Z39.50 (4.6b e 4.6c).
- Compressão da resposta e o XSLT de visualização no navegador (`OAI.xslt` do Koha).
