# UNIMARC (v0)

> Status: v0, tarefa 4.8. Base: [doc 04](../../arquitetura/04-decisoes.md), decisão 4 (importação e exportação em MARC21, MARCXML e UNIMARC); [doc 03](../../arquitetura/03-estrategia-reconstrucao.md), Fase 4, tarefa 4.8; [mapeamento MARC21](mapeamento-marc21.md); [importação](importacao.md); [listagem](listagem.md), itens 33 e 34; [lote](lote.md). Critério da tarefa: **ida e volta sem perdas nos registros de teste**.

O Elzevir lê e escreve UNIMARC bibliográfico pela **projeção MARC21**: o registro UNIMARC vira um registro MARC21 e entra pelo [mapeamento MARC21](mapeamento-marc21.md); na saída, o registro MARC21 da exportação vira UNIMARC. Assim um só mapeamento descreve o modelo BIBFRAME, e uma só tabela por campo diz como as duas famílias se correspondem, lida num sentido para escrever e no outro para ler (`elzevir-app::unimarc`). O núcleo não conhece UNIMARC.

## 1. Comportamento

### Leitura

1. **Reconhecimento.** Um registro é UNIMARC quando tem `200` (título) e não tem `245`. Vale para ISO 2709, MARCXML e as outras leituras da [importação universal](importacao-universal.md). Registro de autoridade (Líder/06 `x`, `y`, `z`) é recusado, como no MARC21.
2. **Conjunto de caracteres.** `100 $a/26-29` declara o conjunto. `01` e `03` são ISO 5426 e são convertidos para UTF-8; `50` é UTF-8; sem declaração, os bytes precisam ser UTF-8 válido. Outro conjunto recusa o registro ("encoding"). O registro como chegou fica guardado sem mudança (`original_records`, família `UNIMARC`), com a declaração e o decodificador usado.

### Dados codificados

3. **Líder.** Situação (05), tipo (06) e nível (07) por tabela: `t`↔`b` (manuscrito), `m`↔`l` (recurso eletrônico), `o` e `p`→`m` (multimídia, que volta como `o`), `b` e `d`→`a` (parte, que volta como `a`); o resto tem o mesmo código. Líder/18: ISBD (`i`) é branco, ISBD parcial (`c`) é `i`, nenhum (branco) é `n`; AACR 2 (`a`) não tem código UNIMARC e vai para `801 $g AACR2`. As posições 20 a 23 saem `4500`, como o `MARC::Record` escreve para o Koha.
4. **Fonte (`040` e `003`) ↔ `801`.** `040 $a` e `$e` vão para `801 #0 $b` e `$g`; cada `$c` para um `801 #1 $b`; cada `$d` para um `801 #2 $b`; `003` (quem atribuiu o `001`) para `801 #3 $b`, que no UNIMARC é a agência distribuidora: não há outro lugar para ele. `040 $b` é a língua de catalogação, em `100 $a/22-24`.
5. **`008` ↔ `100`, `101`, `102`, `105` e `106`.**
   - `100 $a`: data de entrada (`008/00-05` com o século, `yy` abaixo de 65 é 20yy), tipo de data (`008/06`, por tabela), as duas datas (`07-14`), público (`008/22` de livro) e `50` (UTF-8) nas posições 26 a 29.
   - `101 $a`: a língua de `008/35-37` quando não há `041`; com `041`, o `101` vem dele (item 6).
   - `102 $b` com `$2 marccountry`: o país de `008/15-17` no código MARC, que volta exato. O código ISO 3166 de `102 $a` fica para depois (§5).
   - `105 $a` de livro: ilustrações (`008/18-21`, `y` sem nenhuma), natureza do conteúdo (`24-27`, `z` sem nenhuma), congresso, homenagem, índice, forma literária e biografia (`29` a `34`). `106 $a`: forma do item (`008/23`).

### Campos de dados

6. **Uma tabela por par de etiquetas.** Cada campo MARC21 tem uma etiqueta UNIMARC, indicadores e uma lista de pares de subcampos `(MARC21, UNIMARC)`. Na escrita, cada subcampo MARC21 procura o primeiro par com o seu código; na leitura, cada subcampo UNIMARC procura o primeiro par com o seu. Subcampo sem par fica de fora. A tabela:

   | MARC21 | UNIMARC | MARC21 | UNIMARC |
   |---|---|---|---|
   | `020` | `010` | `500` | `300` |
   | `022` | `011` | `504` | `320` |
   | `016` | `020` | `505` | `327` |
   | `035` | `035` | `520` | `330` |
   | `041` | `101` | `534` | `324` |
   | `043` | `660` | `546` | `302` |
   | `045` | `661` | `555` | `321` |
   | `050` | `680` | `561` | `317` |
   | `080` | `675` | `590` | `316` |
   | `082` | `676` | `600` | `600` (família: `602`) |
   | `084` | `686` | `610`, `611` | `601` (1º indicador `0`, `1`) |
   | `100` | `700` (família: `720`) | `630` | `605` |
   | `110`, `111` | `710` (1º indicador `0`, `1`) | `650` | `606` |
   | `130`, `240` | `500` | `651` | `607` |
   | `210` | `531` | `655` | `608` |
   | `243` | `501` | `700` | `701` (família: `721`) |
   | `245` | `200` | `710`, `711` | `711` (1º indicador `0`, `1`) |
   | `246` | `510` a `517` (item 8) | `740` | `488`, ou `464` com 2º indicador `2` |
   | `250` | `205` | `775` | `451` |
   | `260` | `210` | `830` | `410` |
   | `264` | `214` (2º indicador igual) | `856` | `856` |
   | `300` | `215` | `952` | `995` (item 10) |
   | `310` | `326` | `336`, `337`, `338` | `181`, `182`, `183` |
   | `440`, `490` | `225` (item 9) | | |

   Na leitura entram também `702` (como `700`), `712` (como `710` ou `711`) e `722` (como `700` de família), que o Elzevir não escreve.
7. **Nomes.** Nome invertido (1º indicador MARC21 `1`) separa-se na primeira vírgula: `$a` sobrenome e `$b` o resto, com 2º indicador UNIMARC `1`; na leitura, `$b` volta para `$a` depois de ", ". Numeração `$b`↔`$d`, datas `$d`↔`$f`, forma completa `$q`↔`$g`, função `$4`↔`$4`, autoridade `$0`↔`$3`, objeto do mundo real `$1`↔`$1`. Congresso: número `$n`↔`$d`, lugar `$c`↔`$e`, data `$d`↔`$f`. O termo de função `$e` não tem subcampo UNIMARC (item 14).
8. **Títulos.** `245` `$a $b $n $p $c` ↔ `200` `$a $e $h $i $f`; o 1º indicador diz se o título gera entrada. O `246` vai para a etiqueta do seu tipo (2º indicador): paralelo `510`, capa `512`, folha de rosto adicional `513`, cabeçalho `514`, título corrente `515`, lombada `516`, outros `517`; na leitura, cada etiqueta volta com o seu indicador.
9. **Séries.** `490` com 1º indicador `1` (com entrada em `8XX`) é `225` com 1º indicador `0`; `490` sem entrada é `225` `1`; `440` é `225` `2`. `830` `$a $p $v` é `410` `$t $i $v`.
10. **Exemplares.** O `952` do Koha MARC21 e o `995` do Koha UNIMARC, subcampo por subcampo, pelo `kohafield` de cada um nos dois frameworks padrão do Koha (por exemplo, biblioteca `$a`↔`$b`, código de barras `$p`↔`$f`, número de chamada `$o`↔`$k`, tipo `$y`↔`$r`).
11. **Assuntos.** Subdivisões de forma `$v`↔`$j`, geral `$x`↔`$x`, cronológica `$y`↔`$z` e geográfica `$z`↔`$y`. O tesauro do 2º indicador MARC21 vai para `$2` (`0` `lcsh`, `1` `lcshac`, `2` `mesh`, `3` `nal`, `5` `cash`, `6` `rvm`); `4` (não especificado) não escreve `$2`; `7` já o tem. Na leitura, o `$2` volta para o indicador, e `$2` desconhecido dá `7`. As marcas de palavras fora da ordenação (NSB e NSE, como U+0088 e U+0089 ou U+0098 e U+009C) são tiradas de todo subcampo lido.
12. **Ordem.** Os campos saem em ordem de etiqueta; os de mesma etiqueta, na ordem do registro de origem.
13. **Controle.** `001` e `005` são os mesmos nas duas famílias. Os outros campos de controle MARC21 (`006`, `007`) não têm lugar no UNIMARC.
14. **Perdas MARC21 → UNIMARC.** O UNIMARC não tem lugar para: termo de função (`$e`) sem código; título em cabeçalho de assunto (`610 $t`); forma da obra (`380`); modo de publicação (`334`); termos (`$a`) de `336` a `338`, cujos códigos ficam; tipo de título variante porção, distintivo e outro (`246` 2º indicador `0`, `2` e `3`, todos `517`); escrita da nota de língua (`546 $b`); forma literária "romance" (`008/33` `f`, que volta como ficção `1`); público "especializado" (`008/22` `f`, que volta como adulto `e`), e "geral" (`g`) e "adulto" (`e`) ficam distintos. Tudo que o modelo diz e o UNIMARC tem onde dizer vai e volta.

### Escrita

15. **Formatos.** UNIMARC em ISO 2709 (UTF-8, `100 $a/26-29` `50`) e UNIMARC no leiaute do MARCXML (o mesmo `<record>` do MARC21 slim, que o esquema permite; o Líder/09 não se aplica, porque o UNIMARC declara UTF-8 no `100`).
16. **Onde.** O menu "Export" do cartão da Instância ganha UNIMARC e UNIMARC XML (`GET /equipe/acervo/instancia/{id}/marc?format=unimarc|unimarcxml`, arquivo `elzevir-{id}-unimarc.mrc` ou `.xml`), e a seleção de famílias ganha "Exportar UNIMARC" (`/equipe/acervo/lote/marc?format=unimarc`, `elzevir-lote-unimarc.mrc`). `unimarcxml` também vale no lote.
17. **Registro que ninguém mudou.** Um registro lido de UNIMARC, cujo modelo ainda diz o que a sua projeção diz, sai como chegou, em UTF-8 (o ISO 5426 é convertido e o `100 $a/26-29` vira `50`): é a mesma regra do MARC21, que sai com os bytes guardados.
18. **Registro mudado ou nascido no Elzevir.** Sai pela exportação MARC21 e pela tabela (itens 3 a 13).
19. **Campos que a projeção não lê.** De um registro lido de UNIMARC e depois mudado, os campos do original que nenhuma linha da tabela lê (campos locais `9XX`, ligações `4XX` fora da tabela, campos de controle além de `001` e `005`) saem como chegaram, em ordem de etiqueta.
20. **Falha.** Registro que não pode ser escrito fica de fora, com o motivo, como na exportação MARC21 ([lote](lote.md), item 8).

## 2. Origem no Koha

O Koha guarda os registros numa família só, escolhida na instalação (`marcflavour`), e não converte entre MARC21 e UNIMARC: não há oráculo para a tabela. A tabela segue as correspondências de etiqueta e subcampo do UNIMARC Bibliográfico (IFLA) e os frameworks padrão do Koha para as duas famílias (`installer/data/mysql/en/marcflavour/*/mandatory/*_framework_DEFAULT.yml`), que dão os exemplares (`952`↔`995`, item 10). Do Koha vêm também a leitura do conjunto de caracteres pelo `100 $a/26-29` (`C4::Charset`) e as posições 20 a 23 do Líder.

## 3. Testes

- **Ida e volta sem perdas** (critério da tarefa): os 200 registros da exportação da equipe viram registros UNIMARC; cada um é lido, exportado de novo e lido outra vez. O modelo é o mesmo, e os campos e o Líder do UNIMARC exportado são iguais aos do lido. A primeira geração difere só da pontuação ISBD que o escritor MARC21 põe antes de `$e`, que o UNIMARC não tem.
- **MARC21 → UNIMARC:** dos mesmos 200, o teste lista um a um os registros que perdem algo, com o motivo do item 14 (28 registros, a maioria pelo termo de função); qualquer outro registro diferente falha.
- Unidade: nomes invertidos, título, assunto com tesauro, exemplar, `100 $a` e `105 $a`; leitura de `702` e `712`, marcas NSB e NSE e `$2` local.
- Integração com o banco: registro UNIMARC importado sai como chegou em ISO 2709 e em XML; registro ISO 5426 sai em UTF-8; registro editado passa pela tabela e mantém o campo local `930`; lote em UNIMARC XML.

## 4. Desvios

- O `003` vai para `801 #3 $b` (item 4), que não é o seu sentido no UNIMARC.
- `$1` (objeto do mundo real) e `$0` lido como `$0` (de agências que copiam o subcampo MARC21) não são subcampos definidos nos `7XX` do UNIMARC.

## 5. Fora desta versão

- `102 $a` com o código ISO 3166 do país (a tabela MARC ↔ ISO não está no repositório).
- Campos codificados de material não textual (`110` seriados, `115` a `128`, `135`) e o `007`.
- Autoridades UNIMARC/A.
- Exportar o catálogo inteiro em UNIMARC pela linha de comando (`elzevir export`), e a exportação UNIMARC no OPAC.
