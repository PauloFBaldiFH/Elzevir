# Inventário por leitura de código de barras

> Status: tarefa 5.5, parte a (modelo e comando). A parte b (tela, lista da área e CSV) virá em PR próprio. Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 5, linha 5.5).

## 1. Comportamento

1. Cada exemplar guarda o dia em que foi visto pela última vez numa leitura de inventário (`items.last_seen`, migração 67); vazio até a primeira leitura.
2. **Códigos lidos.** O texto de uma leitura é separado em códigos por espaço, quebra de linha, vírgula ou ponto e vírgula. Fica de fora, e é contado à parte: código com caractere de controle, código maior que o de um exemplar (64 caracteres) e código repetido (sem diferença de maiúscula e minúscula). Os códigos ficam na ordem em que foram lidos. Uma leitura tem no máximo 20.000 códigos; mais que isso é recusado.
3. **Área contada.** A área é a combinação dos critérios dados, cada um opcional: unidade (a de origem ou a atual do exemplar), localização, coleção e faixa de número de chamada. A faixa compara pela chave de ordenação Dewey do Koha (item 9); com só o primeiro número, vai até esse número seguido de `Z`, o que pega todo número que começa como ele (`869` pega `869.1` e `869.93`).
4. **Esperados na estante.** Da área, entram na lista do que se espera achar os exemplares que passam pelos filtros dados: não vistos desde um dia (ou nunca vistos), sem os emprestados, sem os separados para reserva.
5. **Leitura.** Cada código lido acha o exemplar sem diferença de maiúscula e minúscula. O exemplar achado, se não estiver baixado, passa a ter `last_seen` no dia da leitura e deixa de estar perdido (a versão do exemplar sobe só quando ele estava perdido, e o registro do OPAC é refeito). O exemplar baixado não é marcado. O emprestado é marcado mas continua emprestado: a devolução é feita no balcão.
6. **Relatório.** Primeiro, na ordem da leitura, os códigos com problema:
   - código que nenhum exemplar tem;
   - exemplar baixado;
   - exemplar emprestado (só isso, mesmo que esteja fora do lugar ou estivesse perdido);
   - exemplar fora da área (item 3), que é o "fora do lugar";
   - exemplar que estava perdido e foi achado;
   - com a verificação de ordem pedida, exemplar cuja chave fica abaixo da do lido antes ou acima da do lido depois (baixados não contam como vizinhos).
   Depois, com a comparação pedida, os esperados (item 4) que não foram lidos, em ordem de estante: "não lido" ou, sem código de barras, "sem código". Cada linha traz exemplar, código, título e número de chamada.
7. Sem nenhum código lido e sem a comparação, o comando é recusado. Só com a comparação, ele devolve a lista dos esperados não lidos sem marcar nada.
8. O comando é `catalog.inventory`, com a permissão de catalogação, e vai inteiro para o `audit_log`, com os códigos lidos.
9. **Chave de ordenação Dewey.** Maiúsculas; `/` vira espaço; um prefixo de letras vira um pedaço próprio; os pedaços se separam por ponto e espaço; o primeiro grupo de algarismos fica como está e o segundo, se vier logo depois, é completado com zeros à direita até 15 algarismos; com um grupo só, ele ganha `_000000000000000`; os pedaços se juntam por `_` e o que não é letra, algarismo ou `_` sai. Para comparar, o `_` vale menos que qualquer letra ou algarismo, como na colação do banco do Koha.

## 2. Origem no Koha

- `tools/inventory.pl` (leitura, marcação de `datelastseen`, perdido achado, problemas `wrongplace`, `checkedout`, `out_of_order`, `not_scanned`, `no_barcode`) e `C4::Items::GetItemsForInventory` (área e filtros).
- `C4::ClassSortRoutine::Dewey::get_class_sort_key` e `t/ClassSortRoutine_Dewey.t`.

## 3. Paridade

`livre` (inventário não está entre as áreas de paridade exigida), com a chave de ordenação igual à do Koha. Prova: testes de unidade em `elzevir-domain::inventory` (os casos de `t/ClassSortRoutine_Dewey.t`, faixa, problemas de cada exemplar lido, não lidos) e em `elzevir-app::inventory` (separação dos códigos), e `crates/elzevir-store/tests/inventory.rs` com o acervo de exemplo do Koha (fora de ordem, perdido achado, fora do lugar, baixado, código desconhecido, não lido, sem código, marcação de visto, filtro "não visto desde", recusa sem códigos).

## 4. Casos de borda

1. O mesmo código lido duas vezes: lido uma vez só, e contado como repetido.
2. Código em minúsculas para exemplar cadastrado em maiúsculas: achado.
3. Exemplar baixado lido: não é marcado e não aparece como "não lido".

## 5. Desvios

- O Koha devolve o exemplar emprestado que é lido no inventário (a menos que se marque `dont_checkin`); o Elzevir só o aponta, para a devolução passar pelo balcão, com multa e reserva.
- O Koha lê duas vezes um código repetido e põe o exemplar baixado lido entre os "não lidos"; o Elzevir lê uma vez e não o põe.
- O Koha guarda data e hora em `datelastseen` e a muda também em empréstimo e devolução; o Elzevir guarda o dia da leitura de inventário.
- O filtro por situação (`notforloan`, `itemlost`, `withdrawn`, `damaged`) e por tipo de item do Koha não entram: a área e os filtros do item 4 bastam para a contagem de estante.
