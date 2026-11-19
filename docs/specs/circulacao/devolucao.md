# Devolução

> Status: tarefa 3.5, parte a (devolução sem multa e sem suspensão). Renovação, suspensão na devolução e a gravação do oráculo vêm nas próximas partes; a multa vem com a conta do leitor (3.7) e a reserva encontrada na devolução com as reservas (3.8). Base: Koha 26.05.05.

## 1. Comportamento

1. O comando `circ.checkin`, com a permissão de circulação, recebe o código de barras, a unidade do balcão e o momento, no fuso da biblioteca. O código é lido sem espaços nas pontas.
2. São recusados, sem gravar nada: código desconhecido (Koha: `BadBarcode`) e exemplar retirado (`withdrawn` diferente de 0, com `BlockReturnOfWithdrawnItems` ligado, o padrão).
3. Com empréstimo, ele termina e passa para os empréstimos encerrados com a data da devolução e a unidade do balcão. O leitor fica gravado só quando o histórico de leitura dele é guardado ([leitores.md](leitores.md), §5). A resposta diz quem tinha o exemplar e se ele estava atrasado (vencimento antes do momento da devolução, Koha: `is_overdue`).
4. Sem empréstimo, a devolução ainda acontece (Koha: `NotIssued`): o exemplar vai para a unidade do balcão e segue as regras de transferência.
5. Um exemplar perdido é encontrado (perdido volta a 0, Koha: `WasLost`). Com `BlockReturnOfLostItems` ligado (padrão desligado), ele continua perdido, o empréstimo continua e não há transferência.
6. O exemplar passa a estar na unidade do balcão.
7. Para onde ele volta vem da regra `returnbranch` da unidade de origem do exemplar e da política dele (Koha: `CircControlReturnsBranch` = `ItemHomeLibrary`, o padrão):
   - sem regra, ou `homebranch`: a unidade de origem;
   - `holdingbranch`: a unidade em que estava antes da devolução;
   - `noreturn`: fica onde foi devolvido;
   - `returnbylibrarygroup`: a unidade de origem, porque o Elzevir não tem grupos de bibliotecas.
8. Transferências:
   - uma transferência aberta para a unidade do balcão chega (Koha: `TransferArrived`);
   - uma transferência aberta para outra unidade continua, e a resposta avisa para onde o exemplar deve ir (`WrongTransfer`);
   - sem transferência aberta, se a unidade de volta é outra, o exemplar é enviado a ela (`AutomaticItemReturn` ligado, o padrão); com ele desligado, a resposta só avisa (`NeedsTransfer`). O motivo é `ReturnToHome` ou `ReturnToHolding`.
9. Os empréstimos encerrados e as transferências não entram no resumo da importação.

## 2. Origem no Koha

- `C4::Circulation::AddReturn`, `MarkIssueReturned`, `CanBookBeReturned` (`AllowReturnToBranch` = `anywhere`, o padrão, aceita qualquer unidade).
- `Koha::CirculationRules::get_return_branch_policy`, `Koha::Item::Transfer::receive`, `ModItemTransfer`.

## 3. Paridade

`exigida`. Prova, por enquanto: `crates/elzevir-domain/src/checkin.rs` (decisão pura) e `crates/elzevir-store/tests/checkout.rs` (devolução pelo comando: volta para a origem, chegada da transferência, regra `noreturn`, atraso, retirado, perdido). A gravação do oráculo vem na parte seguinte.

## 4. Casos de borda

1. Devolver na própria unidade de origem: fica, sem transferência.
2. Devolver um exemplar em trânsito para outra unidade: a transferência continua.
3. Devolver um exemplar perdido sem empréstimo: ele é encontrado e volta para a origem.

## 5. Desvios

Nenhum.
