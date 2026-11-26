# Migração do Koha, parte 2: empréstimos, reservas e contas (v0)

> Status: v0, tarefas 5.2a (empréstimos e reservas, itens 1 a 8) e 5.2b (contas dos leitores, itens 9 a 13) do [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Fase 5). Depois vem o histórico de leitura de quem consentiu (5.2c). Base: [parte 1](leitores-do-koha.md), [empréstimo](../circulacao/emprestimo.md), [renovação](../circulacao/renovacao.md), [reservas](../circulacao/reservas.md), [conta](../circulacao/conta.md) e a [importação do Koha](../catalogo/importacao-universal.md). Critério: **cada empréstimo e cada reserva em curso no Koha chega ao leitor e ao exemplar certos, na ordem da fila, e cada conta chega inteira, com o mesmo saldo, ou aparece no relatório com o motivo**.

O mesmo `elzevir migrate-koha` da parte 1, depois de gravar os leitores, grava os empréstimos e as reservas do dump e, por fim, as contas dos leitores. O acervo precisa ter vindo antes, pela importação com origem e casamento por `biblionumber`: é por ela que o Elzevir sabe qual Instância e qual exemplar correspondem aos números do Koha. Nenhum número do Koha entra no núcleo; tudo é achado pelas tabelas de borda que a importação e a parte 1 preencheram.

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

### Contas

9. A conta inteira de cada leitor vem, linha a linha, paga ou não (decisão de Paulo, 2026-11-26): cada `accountlines` vira um lançamento com o tipo (`debit_type_code` ou `credit_type_code`, exatamente um), `status`, `amount` e `amountoutstanding` em centavos (arredondados à metade para longe do zero, contados no relatório), `date` como dia do lançamento (sem ela, `timestamp`) e `timestamp` como última mudança, no fuso da biblioteca. Linha sem número, leitor, tipo, valor ou data fica de fora.
10. O tipo é um dos que todo Koha instala ([conta.md](../circulacao/conta.md), item 27). Um tipo criado pela biblioteca no Koha chega como `MANUAL` (débito) ou `CREDIT` (crédito), e o relatório conta cada código; o código local não entra no Elzevir. O exemplar (`itemnumber`) e a unidade (`branchcode`) vêm quando estão no Elzevir; senão o lançamento vem sem eles. A multa ligada a um empréstimo em curso (`issue_id`) fica ligada ao empréstimo migrado, para a multa diária continuar a mesma multa em vez de abrir outra.
11. Os movimentos (`account_offsets`) vêm com a conta, ligando os mesmos lançamentos: `CREATE`, `APPLY`, `OVERDUE_INCREASE`, `OVERDUE_DECREASE` e `VOID`, com o valor e o momento. Movimento que não liga nenhum lançamento migrado fica de fora, contado. O saldo de cada lançamento é o do Koha; os movimentos são o histórico dele.
12. A conta de um leitor vai inteira num só comando (`patrons.migrate_koha_accounts`, cerca de 500 lançamentos por comando). Leitor que não veio deixa a conta de fora. Leitor que já tem algum lançamento no Elzevir conta como já migrado e não recebe nada: a migração roda com o servidor parado, logo depois dos leitores, e uma conta só começa com ela.
13. O relatório diz quantas contas, lançamentos e movimentos vieram, quantas já estavam migradas, os leitores cujas contas ficaram de fora, os lançamentos de fora com o `accountlines_id`, os movimentos de fora, os valores arredondados e os tipos locais trazidos como `MANUAL` ou `CREDIT`, com o código e a contagem.

## 2. Testes

- `crates/elzevir-app/tests/koha_migration.rs`: leitura de `issues`, `reserves`, `accountlines` e `account_offsets` do dump fictício, fuso, ordem da fila, tipos locais, centavos e lotes.
- `crates/elzevir-store/tests/koha_migration.rs`: com o acervo fictício importado, empréstimos (por `itemnumber` e por código de barras), reservas na fila, esperando e em trânsito com a transferência, as contas com os movimentos e a multa ligada ao empréstimo, cada motivo de fora e a segunda execução sem duplicar.
- `crates/elzevir/tests/cli.rs`: o relatório do comando, sem dados pessoais.
