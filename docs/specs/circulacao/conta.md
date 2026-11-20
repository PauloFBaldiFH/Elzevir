# Conta do leitor

> Status: tarefa 3.7, partes a (multa lançada na devolução e dívida no empréstimo), b (pagamento e abono) e c (multa diária). Exemplar perdido vem na próxima parte. Base: Koha 26.05.05.

## 1. Comportamento

1. A conta do leitor é uma lista de lançamentos. Cada lançamento é um débito (o leitor deve) ou um crédito (o leitor pagou ou foi abonado), nunca os dois. O valor é inteiro, em centavos. Cada débito guarda quanto ainda falta pagar.
2. Toda mudança num lançamento deixa um registro de movimento: `CREATE` ao lançar, `OVERDUE_INCREASE` ou `OVERDUE_DECREASE` com a diferença ao mudar o valor de uma multa.
3. **Multa na devolução.** Com `CalculateFinesOnReturn` ligado (o padrão), uma devolução atrasada calcula a multa como em [multa.md](multa.md) e a lança na conta como débito `OVERDUE`. O exemplar perdido não é multado.
4. As regras, o calendário e a unidade da multa são os da unidade onde o exemplar estava antes da devolução (`CircControl` = `ItemHomeLibrary` com `HomeOrHoldingBranch` = `holdingbranch`, os padrões do Koha, que multa antes de trazer o exemplar para o balcão).
5. Uma multa ainda aberta do mesmo empréstimo (`UNRETURNED`) é ajustada em vez de duplicada, e só quando os centavos mudam. O que já foi pago continua pago: o saldo da multa muda pela diferença e nunca fica negativo. Uma multa nova de zero não é lançada.
6. Com `MaxFine` preenchido, o leitor nunca deve mais que ele em multas de atraso: a multa nova ou o aumento param no que falta até o teto, contando o que ainda se deve em todas as multas de atraso, inclusive a do empréstimo. No teto, nada muda.
7. Depois do cálculo, a multa aberta do exemplar fica `RETURNED`. Uma multa aberta de valor zero e sem pagamento é apagada.
8. **Dívida no empréstimo.** O que o leitor deve é a soma dos saldos dos débitos; créditos ainda não aplicados não descontam. Acima de `noissuescharge` (padrão 5,00; zero ou vazio não limita), o empréstimo é impossível; com `AllowFineOverride` ligado, pede confirmação. Com `AllFinesNeedOverride` ligado (o padrão), qualquer dívida pede confirmação.
9. A ficha do leitor mostra quanto ele deve, quando deve. A resposta da devolução mostra a multa lançada.
10. **Pagamento e abono.** Na ficha do leitor, com a permissão de circulação, a equipe paga (`PAYMENT`) ou abona (`WRITEOFF`) um valor, marcando ou não os débitos. O valor é digitado com vírgula ou ponto e até duas casas.
11. O valor precisa ser maior que zero e não pode passar do que se deve nos débitos marcados, ou em todos quando nenhum foi marcado (Koha: `error_over` do `paycollect.pl`). Um débito marcado que já não deve nada recusa o pedido.
12. O pagamento vira um crédito do valor (negativo), com movimento `CREATE`. Ele quita primeiro os débitos marcados, do mais antigo ao mais novo, e o que sobra vai para os outros débitos, também do mais antigo ao mais novo, como o `pay` do Koha. Cada débito leva o que deve enquanto o crédito durar; cada quitação deixa um movimento `APPLY` com o valor negativo.
13. A resposta diz quanto foi pago ou abonado e quanto o leitor ainda deve. A ficha lista a conta, do lançamento mais novo ao mais antigo, com o dia, o tipo, o código de barras do exemplar, o valor e o que falta.
14. **Multa diária.** Enquanto o servidor roda, ele recalcula as multas dos empréstimos atrasados ao iniciar e depois a cada hora (o Koha roda `fines.pl` pelo cron, em geral uma vez por dia). Cada empréstimo atrasado de exemplar não perdido tem a multa calculada até o momento e lançada ou ajustada como no item 5; a multa fica `UNRETURNED` e continua crescendo até a devolução, que a fecha (item 7).
15. Rodar mais de uma vez no mesmo dia não muda nada: a multa só muda quando os centavos mudam. Por isso rodar de hora em hora cobra o mesmo que rodar uma vez por dia; só os empréstimos por hora ficam em dia mais cedo.
16. Num dia em que a unidade está fechada, a multa não é recalculada (`ChargeFinesOnClosedDays` desligado, o padrão). As regras, o calendário e a unidade são os do item 4.
17. Os empréstimos são percorridos por leitor e, dentro de cada leitor, na ordem em que foram feitos (o `Getoverdues` do Koha ordena por leitor), o que decide qual multa para no teto do `MaxFine`.
18. O resultado de cada rodada vai para o log do servidor (`fines.charged`, com quantos empréstimos estavam atrasados e quantas multas foram lançadas ou mantidas; `fines.failed` quando falha, e a próxima hora tenta de novo).
19. As contas não entram no resumo da importação.

## 2. Origem no Koha

- `C4::Circulation::AddReturn`, `_CalculateAndUpdateFine`, `_FixOverduesOnReturn`.
- `C4::Overdues::UpdateFine`, `Koha::Account::add_debit`, `Koha::Account::Line::adjust`.
- `misc/cronjobs/fines.pl`, `C4::Overdues::Getoverdues`.
- `Koha::Account::pay`, `add_credit`, `Koha::Account::Line::apply`, `members/paycollect.pl`.
- `C4::Circulation::CanBookBeIssued` (`DEBT`), `Koha::Patron::is_patron_inside_charge_limits`, `Koha::Account::non_issues_charges`.
- Teste: `reference/koha/t/db_dependent/Overdues.t` ("UpdateFine tests").

## 3. Paridade

`exigida`. Prova, por enquanto: `crates/elzevir-domain/src/account.rs` (a sequência de `MaxFine` de `Overdues.t`, leitura das regras, dívida) `crates/elzevir-store/tests/checkout.rs` (multa lançada na devolução, confirmação e recusa por dívida, pagamento do mais antigo, abono dos marcados, limites do valor, multa diária crescendo e parada no dia fechado) e `crates/elzevir/tests/patrons.rs` (a conta na ficha, pagamento e abono pelo formulário). `crates/elzevir/tests/checkin_oracle.rs` reproduz `scenarios/circulacao/multa-conta.koha.json` (gravado por `tools/fines-recorder/record.py`): duas devoluções atrasadas com fim de semana no atraso dão as mesmas multas que o Koha, a dívida pede a mesma confirmação (`DEBT`), o pagamento sem linhas marcadas quita na mesma ordem e o abono da linha marcada deixa os mesmos saldos. O `fines.pl` não roda pela web, então a multa diária é comparada só pelo cálculo, que é o mesmo da devolução.

## 4. Casos de borda

1. Devolução no vencimento: nenhum lançamento.
2. Dívida igual ao limite: pede confirmação, não recusa (o Koha compara com "maior que").
3. Valor de regra com mais de duas casas: arredondado para o centavo mais próximo (`0.005` vira 0,01).
4. Pagamento do valor exato da dívida: quita tudo e o crédito fica sem saldo.
5. Valor digitado com três casas ou com separador de milhar: recusado antes de chegar ao escritor.

## 5. Configuração

O Elzevir não tem `finesMode`: a multa é cobrada quando a regra tem valor de multa. Uma biblioteca que não cobra multas deixa `fine` vazio. As demais preferências seguem os padrões do Koha até chegarem ao `elzevir.toml`.

## 6. Fora desta parte

- Tipo de pagamento, caixa e recibo (`RequirePaymentType`, `UseCashRegisters`, `AutomaticEmailReceipts`); troco; estorno e devolução de crédito (`payout`, `OVERPAYMENT`).
- Débitos manuais e créditos avulsos; a permissão própria de cobrança (Koha: `updatecharges`), que hoje é a de circulação.
- Exemplar perdido por atraso longo (`longoverdue.pl`).
- As opções do `fines.pl` (`--maxdays`, `--verify-issue`, arquivo de relatório).
- Isenção da multa na devolução (`exemptfine`) e devolução com data retroativa.
- Limites de dívida da categoria e de responsáveis (`NoIssuesChargeGuarantees`).
