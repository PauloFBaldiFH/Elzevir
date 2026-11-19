# Suspensão na devolução atrasada

> Status: tarefa 3.5, partes c e d. Base: Koha 26.05.05.

## 1. Comportamento

1. Uma devolução atrasada ([devolucao.md](devolucao.md), item 3) pode suspender o leitor. As regras de circulação vêm da unidade do balcão (o exemplar já está nela quando o Koha calcula), da categoria do leitor e da política do exemplar; o calendário é o da mesma unidade.
2. Sem `finedays`, ou com `finedays` 0, não há suspensão.
3. Os dias de atraso contam como na multa ([multa.md](multa.md)): com `finesCalendar` = `noFinesWhenClosed` (o padrão), dias fechados não contam. Se os dias de atraso não passam da carência (`firstremind`), não há suspensão.
4. A suspensão é `dias de atraso × finedays`. Com `suspension_chargeperiod` maior que 1, ela é dividida por ele, arredondando para baixo. Com `maxsuspensiondays` preenchido, ela não passa dele.
5. Ela conta a partir da data da devolução. Com `CumulativeRestrictionPeriods` ligado (padrão desligado) e uma suspensão em curso, conta a partir do fim dela.
6. Com `SuspensionsCalendar` = `noSuspensionsWhenClosed` (o padrão), só dias abertos contam, como no vencimento com `useDaysMode` = `Calendar`; com `ignoreCalendar`, todos os dias contam.
7. O leitor tem no máximo uma suspensão. Uma nova suspensão só a estende: se a atual termina depois, fica a atual. Uma suspensão sem fim é substituída pela nova data, como no Koha, que só compara duas datas.
8. A suspensão vale até o dia anterior ao fim: no dia do fim o leitor já pode emprestar (Koha: `is_debarred` compara a data com o momento atual).
9. A resposta da devolução diz até quando o leitor fica suspenso, quando a devolução o suspendeu.
10. O empréstimo a um leitor suspenso é impossível (Koha: `DEBARRED` e `USERBLOCKEDWITHENDDATE`, ou `USERBLOCKEDNOENDDATE` sem fim). A recusa vem antes da de cadastro expirado.
11. As restrições do leitor não entram no resumo da importação.

## 2. Origem no Koha

- `C4::Circulation::AddReturn`, `_debar_user_on_return`, `_calculate_new_debar_dt`, `_GetCircControlBranch` (`CircControl` = `ItemHomeLibrary`, `HomeOrHoldingBranch` = `holdingbranch`).
- `Koha::Patron::Debarments::AddUniqueDebarment`, `Koha::Patron::is_debarred`.
- `C4::Circulation::CanBookBeIssued` (`DEBARRED`, `USERBLOCKEDWITHENDDATE`, `USERBLOCKEDNOENDDATE`).

## 3. Paridade

`exigida`. Prova, por enquanto: `crates/elzevir-domain/src/suspension.rs` (cálculo: carência, período, máximo, cumulativa, calendário) e `crates/elzevir-store/tests/checkout.rs` (devolução atrasada suspende, empréstimo recusado até o fim, nova suspensão não encurta a atual) e `crates/elzevir/tests/checkin_oracle.rs`, que reproduz `scenarios/circulacao/devolucao-renovacao.koha.json`: devolução quatro dias depois do vencimento, com fim de semana e feriados no atraso e na suspensão, dá o mesmo fim que o Koha, e o empréstimo seguinte é recusado com `DEBARRED` e `USERBLOCKEDWITHENDDATE`.

## 4. Casos de borda

1. Devolução atrasada só em dias fechados: nenhum dia de atraso, nenhuma suspensão.
2. Suspensão que termina hoje: o leitor empresta.
3. Duas devoluções atrasadas no mesmo dia: fica a suspensão mais longa.

## 5. Fora desta parte

- Empréstimos por hora não suspendem ainda: o Koha mistura horas e dias nesse cálculo (`DateTime::Duration` com `finedays × 24`), e isso ainda não foi conferido no oráculo.
- `RestrictionBlockRenewing` (padrão desligado): a suspensão não impede a renovação.
- Restrições manuais, retirada da suspensão pela equipe e `AutoRemoveOverduesRestrictions` vêm com a ficha do leitor.

## 6. Desvios

Nenhum.
