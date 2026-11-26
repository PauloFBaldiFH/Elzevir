# Migração do Koha, parte 2: empréstimos e reservas em curso (v0)

> Status: v0, tarefa 5.2a do [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Fase 5). Depois vêm os débitos e créditos (5.2b) e o histórico de leitura de quem consentiu (5.2c). Base: [parte 1](leitores-do-koha.md), [empréstimo](../circulacao/emprestimo.md), [renovação](../circulacao/renovacao.md), [reservas](../circulacao/reservas.md) e a [importação do Koha](../catalogo/importacao-universal.md). Critério: **cada empréstimo e cada reserva em curso no Koha chega ao leitor e ao exemplar certos, na ordem da fila, ou aparece no relatório com o motivo**.

O mesmo `elzevir migrate-koha` da parte 1, depois de gravar os leitores, grava os empréstimos e as reservas do dump. O acervo precisa ter vindo antes, pela importação com origem e casamento por `biblionumber`: é por ela que o Elzevir sabe qual Instância e qual exemplar correspondem aos números do Koha. Nenhum número do Koha entra no núcleo; tudo é achado pelas tabelas de borda que a importação e a parte 1 preencheram.

## 1. Comportamento

### Como os números do Koha são achados

1. O leitor, pelo `borrowernumber` que a parte 1 guardou. O exemplar, pelo `itemnumber` que a importação guardou (952$9); quando nenhum ou mais de um exemplar o tem, pelo código de barras que `items.barcode` dá a esse `itemnumber`. A Instância, pelo `biblionumber` (999$c) que a importação guardou. A unidade, pelo código. Nada achado, ou achado mais de uma vez, deixa a linha de fora com o motivo (item 6).

### Empréstimos

2. Cada linha de `issues` vira um empréstimo em curso: `issuedate` como saída, `date_due` como vencimento, os dois no fuso da biblioteca; `branchcode` como unidade do empréstimo; `renewals_count` e `lastreneweddate` como renovações; `onsite_checkout` como empréstimo local. Linha sem `issue_id`, leitor, exemplar, saída, vencimento ou unidade fica de fora como incompleta.
3. O empréstimo é gravado como o Koha o tinha, sem passar pelas regras de circulação (limite, bloqueio, vencimento calculado): a regra vale para os empréstimos novos, e o que o Koha já emprestou continua emprestado. O exemplar sobe de versão, para o OPAC mostrá-lo emprestado. Exemplar que já tem empréstimo no Elzevir não recebe outro: se é do mesmo leitor, a linha conta como já migrada; se é de outro, fica de fora.

### Reservas

4. Cada linha de `reserves` vira uma reserva, conforme `found`:

   | Koha | Elzevir |
   |---|---|
   | vazio | na fila da Instância, na posição de `priority` |
   | `W`, `P` | separada e esperando na unidade de retirada desde `waitingdate` (sem ela, `reservedate`), até `expirationdate` |
   | `T` | separada e em trânsito para a unidade de retirada |

   `reservedate` é o dia da reserva (meia-noite no fuso da biblioteca como hora de criação), `branchcode` a unidade de retirada, `item_level_hold` se o leitor pediu aquele exemplar, `itemnumber` o exemplar pedido ou separado. Reserva separada sem exemplar fica de fora como incompleta.
5. As reservas são gravadas na ordem do Koha: por registro, primeiro as separadas, depois pela posição na fila, depois pelo número da reserva; cada uma da fila entra no fim da fila da Instância, e a fila é renumerada como numa reserva nova. A reserva em trânsito abre a transferência do exemplar, de onde ele está para a unidade de retirada, como ao separar no balcão. Leitor que já tem reserva da Instância não recebe outra: a linha conta como já migrada.
6. Desvios conscientes, contados no relatório: a reserva suspensa no Koha (`suspend`) chega ativa, porque o Elzevir ainda não suspende reservas; a data final de uma reserva na fila (`expirationdate` sem `found`) é descartada, porque a reserva do Elzevir na fila não expira. Transferências que não são de reserva não são migradas.

### Escrita e relatório

7. São no máximo 500 empréstimos e 500 reservas por comando de escrita (`patrons.migrate_koha_circulation`, com registro no `audit_log`). Rodar de novo não duplica nada (itens 3 e 5).
8. O relatório segue o da parte 1, só com contagens e números do Koha: empréstimos migrados e deixados de fora, com o `issue_id` e o motivo; reservas migradas e deixadas de fora, com o `reserve_id` e o motivo, e as contagens do item 6; quantos já estavam migrados.

## 2. Testes

- `crates/elzevir-app/tests/koha_migration.rs`: leitura de `issues` e `reserves` do dump fictício, fuso, ordem da fila e lotes.
- `crates/elzevir-store/tests/koha_migration.rs`: com o acervo fictício importado, empréstimos (por `itemnumber` e por código de barras), reservas na fila, esperando e em trânsito com a transferência, cada motivo de fora e a segunda execução sem duplicar.
- `crates/elzevir/tests/cli.rs`: o relatório do comando, sem dados pessoais.
