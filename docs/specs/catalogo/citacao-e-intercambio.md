# Formatos de citação e intercâmbio (v0)

> Status: v0, tarefa 4.9. Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md), Fase 4, tarefa 4.9; [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §11; [listagem](listagem.md), itens 33 a 36; [OAI-PMH](../opac/oai-pmh.md), item 16 (o Dublin Core); [depósito digital](../repositorio/deposito.md). Critério da tarefa: **a Instância exporta nos quatro formatos**.

A Instância sai em RIS, BibTeX, MODS e Dublin Core, para gerenciadores de referências (Zotero, Mendeley, EndNote) e para outros catálogos. Os quatro saem do registro MARC21 que a exportação já escreve, como no Koha: uma regra de conversão para a equipe e para o OPAC.

## 1. Comportamento

### Onde

1. **Equipe:** o menu "Export" do cartão da Instância ([listagem](listagem.md), item 33) ganha RIS, BibTeX, MODS e Dublin Core. `GET /equipe/acervo/instancia/{id}/citacao?format=ris|bibtex|mods|dc` exige sessão da equipe.
2. **OPAC:** a página do registro ganha "Salvar registro", um menu (`<details>`, sem JavaScript) com os mesmos quatro formatos. `GET /record/{id}/export?format=ris|bibtex|mods|dc` é público. Instância oculta no OPAC, ou que não existe, é 404, como a página do registro ([ocultar no OPAC](ocultar-no-opac.md), item 3).
3. Sem `format`, ou com formato desconhecido, é 400. O registro vem como anexo:

   | Formato | Arquivo | Tipo |
   |---|---|---|
   | RIS | `elzevir-{id}.ris` | `application/x-research-info-systems` |
   | BibTeX | `elzevir-{id}.bib` | `application/x-bibtex` |
   | MODS | `elzevir-{id}-mods.xml` | `application/mods+xml` |
   | Dublin Core | `elzevir-{id}-dc.xml` | `application/xml` |

   Todos em UTF-8 (`charset=utf-8`).
4. O registro de partida é o da exportação MARC21 ([listagem](listagem.md), item 35), lido de volta. Registro que não pode ser escrito em MARC é 500 na equipe, com o motivo, e 404 no OPAC, sem detalhe para o visitante.

### Depósito digital

5. Se a Instância tem depósito publicado ([depósito](../repositorio/deposito.md), etapa 4a), o endereço da página estável dele entra no registro: `UR` no RIS (depois dos `856`), `url` no BibTeX (quando o registro não tem `856 $u`), `<location><url>` no MODS e `dc:identifier` no Dublin Core. O endereço é absoluto, montado com o host pedido, como os IRIs do BIBFRAME ([BIBFRAME completo](bibframe-completo.md), item 28b): `https://{host}/repository/{depósito}` (`http` só sem cookies seguros). Host com caractere que nenhum nome de host tem fica de fora, e o registro sai sem o endereço. Depósito em rascunho, retirado ou de Instância oculta não entra.

### RIS

6. Linhas `XX  - valor` terminadas em CRLF, nesta ordem, como `C4::Ris::marc2ris` com MARC21:
   - `TY` pelo líder: posição 06 `a` com 07 `m` dá `BOOK`; `a` com 07 `a`, `c` ou `d` dá `GEN`; `a` com `b` ou `s` dá `SER`; `a` com `i` dá `DATA`; senão pela 06: `c` e `d` `MUSIC`, `e` e `f` `MAP`, `g` `ADVS`, `i` e `j` `SOUND`, `k` e `r` `ART`, `m` `DATA`, `o` e `p` `GEN`, `t` `MANSCPT`; outro valor, `GEN`;
   - `AU` para cada `100`, depois cada `700`: com primeiro indicador `1` (sobrenome), o `$a` sem espaço depois de vírgula e ponto, sem pontuação final e sem o ponto depois de palavras de duas letras ou mais, seguido de `, $b` quando há `$b`; com outro indicador, o `$a` como está;
   - `ED` com o `$a` de cada `110`, `111`, `710` e `711`;
   - `TI` com o `245 $a` sem a pontuação final (`/ : ; .`) e, havendo `$b`, `: ` e o `$b` sem ela;
   - `T2` com o `490 $a` do primeiro `490`, sem a pontuação final;
   - `SN` com o `020 $a` do primeiro `020` e `SN` com os nove primeiros caracteres do `022 $a` do primeiro `022`;
   - `AV` com `050 $a $b` e `U1` com `082 $a $2`, dos primeiros;
   - da publicação, o primeiro `264`, ou, sem nenhum, o primeiro `260`: um `PY` por `$c` (o ano de quatro dígitos seguido de `///` e do resto; sem ano, `0000///` e o texto), depois `CY` com os `$a` e `PB` com os `$b`, cada um sem a pontuação final e juntos por `, `;
   - `KW` com cada palavra-chave dos `600`, `610`, `611`, `630`, `650`, `651`, `653` a `658`, sem repetir: do `600`, o nome como no `AU`; dos outros, cada subcampo de letra ou `2` a `4`, fora `$i` e `$w`, sem a pontuação final;
   - `N1` com as notas `500` a `535` (os subcampos que o Koha lê em cada uma) juntas por `; `;
   - `N2` com `$a` e `$b` de cada `520`, juntos por `; `;
   - `UR` com o `$u` de cada `856`, e o depósito (item 5);
   - `ER  - ` fecha o registro.
7. Linha sem valor não sai (o Koha escreve `AU  - ` vazio para nome com indicador `2` e `ED  - ` para campo sem `$a`).

### BibTeX

8. Uma entrada `@book{` com a chave igual ao número da Instância, como `C4::Record::marc2bibtex` com MARC21 e sem campos adicionais. Cada campo numa linha, `\tnome = {valor}`, separados por `,`, na ordem: `author` (o primeiro `$a` do primeiro `100`, `110`, `111`, `700`, `710` e `711`, juntos por ` and `), `title` (`245 $a`), `editor` (`260 $f`), `publisher` (`264 $b`, senão `260 $b`), `year` (`264 $c`, senão `260 $c`, senão `260 $g`), `series` (`490 $a`), `address` (`264 $a`, senão `260 $a`), `edition` (`250 $a`), `note` (`500 $a`) e `url` (`856 $u`, senão o depósito). Campo vazio não sai. Os valores saem como estão no registro, como no Koha.
9. Chave `{` ou `}` num valor sai precedida de `\`, para não fechar o campo antes da hora (o Koha não escapa).

### MODS

10. Um documento MODS 3.1 (`<mods version="3.1">` no namespace `http://www.loc.gov/mods/v3`), pelas regras do `MARC21slim2MODS3-1.xsl` do Koha para o que segue, na ordem dele:
    - `titleInfo` do `245` (`nonSort` pelos caracteres do segundo indicador, `title`, `subTitle` do `$b`, `partNumber` do `$n`, `partName` do `$p`); `type="alternative"` do `246` e do `740`; `type="uniform"` do `130`, `240` e `730`;
    - `name` do `100`, `700` (`personal`), `110`, `710` (`corporate`), `111`, `711` (`conference`), com `namePart`, a data em `namePart type="date"`, `role` com `creator` nos `1XX` e `roleTerm` do `$e` (texto) e do `$4` (código `marcrelator`);
    - `typeOfResource` pelo líder 06, com `collection="yes"` no líder 07 `c` e `manuscript="yes"` nos tipos manuscritos;
    - `genre` do `655`;
    - `originInfo`: país do `008/15-17` (`marccountry`), lugar, editora e data do `260`, ou, sem `260`, do `264` com segundo indicador `1`; data do `008/07-10` quando difere; `edition` do `250 $a`; `issuance` (`monographic` ou `continuing`) pelo líder 07;
    - `language` do `008/35-37` e dos `041 $a`;
    - `physicalDescription` com `extent` do `300`;
    - `abstract` do `520`, `tableOfContents` do `505`, `targetAudience` do `521`;
    - `note`: a indicação de responsabilidade (`245 $c`), o `500` e as outras notas `5XX` que o XSL lê;
    - `subject` dos `600`, `610`, `611`, `630`, `650`, `651` e `653`, com a autoridade pelo segundo indicador (`lcsh`, `mesh`, …, ou o `$2`), e `$v`/`$x` como `topic`, `$y` como `temporal` e `$z` como `geographic`;
    - `classification` do `050` (`lcc`), `060` (`nlm`), `080` (`udc`), `082` (`ddc`, com a edição) e `084` (o `$2`);
    - `relatedItem type="series"` do `440`, `490` (primeiro indicador `0`), `800`, `810`, `811` e `830`;
    - `identifier` do `020` (`isbn`), `022` (`issn`), `024` (`isrc`, `ismn`, `upc` pelo indicador) e `010` (`lccn`);
    - `location` com `url` de cada `856 $u` e do depósito (item 5);
    - `accessCondition` do `506` e do `540`;
    - `recordInfo`: `recordContentSource` do `040 $a`, `recordCreationDate` do `008/00-05`, `recordChangeDate` do `005`, `recordIdentifier` do `001` (com `source` do `003`) e `languageOfCataloging` do `040 $b`.
11. A pontuação final sai como no XSL (`chopPunctuation`: `. : , ; /` e espaço no fim).

### Dublin Core

12. O Dublin Core simples do OAI-PMH ([OAI-PMH](../opac/oai-pmh.md), item 16): um documento `oai_dc:dc`, igual ao `format=oaidc` do Koha.

## 2. Origem no Koha

`opac/opac-export.pl` (formatos `ris`, `bibtex`, `mods` e `dc`) e o "Salvar registro" do OPAC (`opac-detail-sidebar.inc`, preferência `OpacExportOptions`); `C4::Ris::marc2ris`, `C4::Record::marc2bibtex`, `C4::Record::marc2modsxml` (`MARC21slim2MODS3-1.xsl`) e `C4::Record::marc2dcxml` (`MARC21slim2OAIDC.xsl`). Na equipe, o Koha exporta pelo `catalogue/export.pl`, com os mesmos conversores. Diferenças intencionais:

- **Formatos fixos.** Os quatro sempre aparecem; não há `OpacExportOptions`, `RisExportAdditionalFields` nem `BibtexExportAdditionalFields`. EndNote, ISBD e MARC-8 ficam de fora.
- **RIS:** nenhuma linha vazia (item 7); `TY  - GEN` quando o líder não dá tipo (o Koha não escreve `TY` para livro com nível bibliográfico fora da tabela).
- **BibTeX:** chaves escapadas (item 9).
- **MODS:** só as partes do item 10 (o XSL tem também cartografia, `007`, `form`, `genre` pelo `008` e itens relacionados `76X` a `78X`). O `264` entra na publicação, que o XSL de 2006 não conhece; e a `location` lê o `856`, onde o XSL do Koha lê o `050` por engano e nunca escreve URL.
- **Depósito:** o endereço do depósito publicado (item 5) é do Elzevir.

## 3. Testes

- Unidade, por formato, com registros montados: cada linha do RIS, cada campo do BibTeX, cada elemento do MODS do item 10, a pontuação e os casos de borda (sem `245`, sem publicação, líder sem tipo, chaves).
- Snapshots (`insta`) dos quatro formatos para registros da exportação de 200 registros da equipe.
- Web: as rotas da equipe e do OPAC, formato desconhecido (400), Instância oculta no OPAC (404), sem sessão na equipe (redireciona) e o endereço do depósito publicado.
