# Um campo para tudo: bipe de código de barras e de ISBN (v0)

> Status: v0, tarefa 2.8. Base: [spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md) §6.3 e §20 item 1, [listagem](listagem.md), [casamento](casamento.md) (chave de ISBN), [exemplares](exemplares.md) e [criação](criacao.md). Critério da tarefa: **ISBN bipado de livro já existente leva à grade dele**.

O trabalho mais frequente do balcão é acrescentar exemplar a um livro que a biblioteca já tem. Com o livro na mão, a pessoa bipa o ISBN da contracapa (ou o código de barras de um exemplar) no campo de busca e cai na grade de exemplares certa.

## 1. Comportamento

### A regra, numa função só

1. A busca da listagem (`GET /equipe/acervo?q=`) passa primeiro por uma função pura da camada app, `listing::scan`, que decide o que o texto é. A regra fica só nela; a tela obedece.
2. Só a primeira página de uma busca passa pela regra (um pedido com `from` é a continuação de uma busca de texto).
3. O texto é considerado **um código** quando, sem os espaços das pontas, tem de 1 a 64 caracteres e nenhum espaço no meio. Fora disso é busca de texto.
4. Ordem da decisão para um código:
   1. **Código de barras de um exemplar** (igual, caractere por caractere): abre a família desse exemplar, com a grade da Instância dele e o cursor na linha do exemplar.
   2. **ISBN válido já cadastrado**: abre a família da Instância, com a grade dela. O ISBN é válido quando tem só dígitos, hifens e `X`, e o dígito verificador confere (ISBN-10 ou ISBN-13 com prefixo 978 ou 979). A comparação usa a chave de casamento ([casamento](casamento.md)): ISBN-10 e ISBN-13 do mesmo livro se encontram, com ou sem hifens. Com mais de uma Instância com o mesmo ISBN, vale a mais antiga, como no casamento.
   3. **ISBN válido que a base não tem**: a busca de texto roda normalmente e, para quem cataloga, abre "+ Nova família" já com o ISBN (normalizado em 13 dígitos) no campo de identificadores.
   4. Qualquer outro código: busca de texto, como antes (tombo, nº de chamada, ISBN com dígito errado).
5. O código de barras vem antes do ISBN porque um exemplar pode ter como código de barras um número que também é um ISBN válido; o exemplar é o achado mais específico.

### Tela

6. A resposta de um código achado é a própria listagem, com uma família só, já aberta e com a Instância marcada como encontrada. A contagem diz "1 registro".
7. Quem cataloga recebe a grade de exemplares aberta ([exemplares](exemplares.md), item 17); com código de barras, o cursor vai para a linha daquele exemplar. Quem não cataloga vê a família aberta, sem grade.
8. O servidor diz o que achou em atributos de `#families` (`data-scan`, `data-scan-instance`, `data-scan-item`, `data-scan-isbn`); o script da página abre a grade ou o formulário. Nada disso é decidido no JavaScript.
9. Com mudança não gravada em outra grade ou formulário, vale a pergunta de sempre antes de trocar ([edição](edicao.md), item 32).
10. `GET /equipe/acervo/nova?isbn=` devolve o formulário da família nova com o ISBN preenchido; o valor passa pela mesma validação (ISBN inválido é ignorado). O que é herdado do registro anterior ([criação](criacao.md), item 17) continua valendo.

### Leitura

11. A camada app pede à porta da listagem (`CatalogListing::scanned`) o dono de um código: a cabeça da família (Hub ou Obra), a Instância e, para código de barras, o exemplar. A leitura é direta na base, pelo índice único de `items.barcode` e pela chave de `match_keys`; não depende do índice de busca, então um exemplar gravado há um segundo já é achado.

## 2. Origem no Koha

- Busca do catálogo por código de barras e por ISBN: `catalogue/search.pl` (índices `bc` e `nb`), que mostra a lista de resultados; para um resultado só, vai ao detalhe (`catalogue/detail.pl`).
- Normalização de ISBN: `C4::Koha::NormalizeISBN` e `GetVariationsOfISBN`.
- "Acrescentar exemplar": `cataloguing/additem.pl?biblionumber=`.

## 3. Paridade

`livre` (interface e busca). Diferença intencional: o Koha leva ao detalhe do registro; o Elzevir leva direto à grade de exemplares, que é onde o trabalho acontece.

Testes: `crates/elzevir-app/src/listing.rs` (a regra, caso a caso), `crates/elzevir-store/tests/listing.rs` (leitura por código de barras e por ISBN-10/13 sobre os registros do oráculo) e `crates/elzevir-web/tests/staff_catalog.rs` (atributos da página e formulário com ISBN).

## 4. Fora desta versão

- Rascunhos de fontes externas (Z39.50, SRU, IA) para o ISBN que a base não tem: Fase 4, atrás da porta `DraftSource` (spec de interação §17).
- Aviso de duplicata ao digitar um ISBN já cadastrado dentro do formulário de família nova (§6.3 item 5): hoje o bipe na busca resolve antes de o formulário abrir.
- ISSN bipado (periódicos).
