# Conta do leitor

> Status: tarefa 3.7, parte a (multa lançada na devolução e dívida no empréstimo). Pagamento, abono, multa diária pelo agendador e exemplar perdido vêm nas próximas partes. Base: Koha 26.05.05.

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
10. As contas não entram no resumo da importação.

## 2. Origem no Koha

- `C4::Circulation::AddReturn`, `_CalculateAndUpdateFine`, `_FixOverduesOnReturn`.
- `C4::Overdues::UpdateFine`, `Koha::Account::add_debit`, `Koha::Account::Line::adjust`.
- `C4::Circulation::CanBookBeIssued` (`DEBT`), `Koha::Patron::is_patron_inside_charge_limits`, `Koha::Account::non_issues_charges`.
- Teste: `reference/koha/t/db_dependent/Overdues.t` ("UpdateFine tests").

## 3. Paridade

`exigida`. Prova, por enquanto: `crates/elzevir-domain/src/account.rs` (a sequência de `MaxFine` de `Overdues.t`, leitura das regras, dívida) e `crates/elzevir-store/tests/checkout.rs` (multa lançada na devolução, confirmação e recusa por dívida). A gravação no oráculo vem com a multa diária.

## 4. Casos de borda

1. Devolução no vencimento: nenhum lançamento.
2. Dívida igual ao limite: pede confirmação, não recusa (o Koha compara com "maior que").
3. Valor de regra com mais de duas casas: arredondado para o centavo mais próximo (`0.005` vira 0,01).

## 5. Configuração

O Elzevir não tem `finesMode`: a multa é cobrada quando a regra tem valor de multa. Uma biblioteca que não cobra multas deixa `fine` vazio. As demais preferências seguem os padrões do Koha até chegarem ao `elzevir.toml`.

## 6. Fora desta parte

- Pagamento, abono e a ordem em que um pagamento quita os débitos (`Koha::Account::pay`).
- Multa diária pelo agendador (`fines.pl`), exemplar perdido por atraso longo (`longoverdue.pl`).
- Isenção da multa na devolução (`exemptfine`) e devolução com data retroativa.
- Limites de dívida da categoria e de responsáveis (`NoIssuesChargeGuarantees`).
