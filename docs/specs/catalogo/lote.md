# Ações em lote na listagem

> Status: tarefa 2.17, parte a (seleção e exportação). Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 2, linha 2.17), [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §20 itens 8 e 11, [listagem](listagem.md) e [facetas](facetas.md). Alterar exemplares em lote é a parte b; mandar a seleção para uma lista de trabalho com nome é a parte c.

## 1. Comportamento

### Seleção (parte a)

1. Cada linha-família da listagem tem uma caixa de seleção antes do ícone. Clicar nela marca ou desmarca a família sem abrir a linha.
2. A seleção é da aba do navegador: vale entre páginas, buscas, filtros e listas de trabalho, e some ao fechar a aba (`sessionStorage`). Linhas que chegam pelo carregamento da página seguinte já vêm marcadas se estavam na seleção.
3. Com ao menos uma família marcada, aparece abaixo da faixa uma barra com o número de famílias selecionadas, as ações e "Limpar seleção". Sem seleção, a barra fica escondida.
4. Valem até 500 famílias por seleção. A caixa de quem passaria disso não marca, e a barra avisa do limite.

### Exportar a seleção (parte a)

5. A barra exporta a seleção em MARC21 (ISO 2709) ou MARCXML, para qualquer pessoa da equipe, como a exportação de um registro ([listagem](listagem.md), itens 33 a 36).
6. `GET /equipe/acervo/lote/marc?format=marc21|marcxml&head=h12&head=w7` exige sessão da equipe. `head` é a família como a listagem a escreve: `h` e o número da Matriz, ou `w` e o número da Obra. Família repetida conta uma vez; malformada é ignorada; passando de 500, as que sobram são ignoradas. Sem nenhuma família válida, ou com formato desconhecido, é 400.
7. Saem todas as Instâncias de cada família, na ordem da seleção, e dentro da família na ordem da listagem (Obra, depois Instância). Cada registro sai pela mesma regra da exportação de um registro e da linha de comando. O MARCXML é uma coleção (`<collection>`), como a exportação em lote do Koha.
8. O arquivo vem como anexo, `elzevir-lote.mrc` (`application/marc`) ou `elzevir-lote.xml` (`application/marcxml+xml`). Registro que não pode ser escrito fica de fora, e o cabeçalho `X-Elzevir-Not-Exported` lista as Instâncias que ficaram. Família que deixou de existir é ignorada.

## 2. Origem no Koha

O Koha junta registros no carrinho (`opac-basket.pl`, `basket/basket.pl`) e nas listas (`virtualshelves/shelves.pl`) e exporta por `basket/downloadcart.pl` e `virtualshelves/downloadshelf.pl`, com `Koha::Exporter::Record::export` (MARC21, MARCXML e outros). No Elzevir a seleção é de famílias, sem carrinho guardado no servidor; listas guardadas são a parte c.

## 3. Paridade

`livre` (interface). Prova:

- `elzevir-app` (unidade): seleção lida dos parâmetros, sem repetição, com limite; Instâncias de cada família na ordem; MARCXML de vários registros numa coleção.
- `crates/elzevir-web/tests/staff_catalog.rs`: caixa em cada linha, barra escondida, exportação exige sessão, formato e seleção vazia dão 400.
- `crates/elzevir/tests/opac.rs`: a exportação da seleção sobre a base de testes dá os mesmos bytes que a exportação de cada registro, em sequência.
