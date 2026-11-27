# Renovação

> Status: tarefa 3.5, partes b e d; reservas e saldo devedor entraram na tarefa 3.10c. A suspensão não bloqueia no balcão (`RestrictionBlockRenewing` desligado). Base: Koha 26.05.05.

## 1. Comportamento

1. O comando `circ.renew`, com a permissão de circulação, recebe o código de barras, o momento (no fuso da biblioteca) e se a equipe passa por cima dos limites (Koha: `override_limit`).
2. Recusas, nesta ordem e sem gravar nada:
   - código desconhecido; exemplar sem empréstimo (Koha: `no_checkout`);
   - empréstimo local (`onsite_checkout`), mesmo passando por cima dos limites;
   - sem renovações sobrando (`too_many`): `renewalsallowed` ausente, vazio, zero ou não maior que as renovações já feitas, lido como número do Perl;
   - atraso (`overdue`), conforme `OverduesBlockRenewing`: `allow` (padrão) não bloqueia, `block` bloqueia se algum empréstimo do leitor está atrasado, `blockitem` só se este está;
   - saldo devedor (`too_much_owing`): o saldo do leitor, débitos menos créditos ainda não usados (`FineNoRenewalsIncludeCredits` ligado), acima de 100 na moeda (`FineNoRenewals`, fixo no padrão do Koha);
   - reserva (`on_reserve`): uma reserva neste exemplar que ainda não foi atendida, ou uma reserva que o exemplar atenderia, pelas mesmas regras da devolução (`CheckReserves`). Qualquer reserva assim bloqueia, mesmo que outro exemplar do registro esteja livre (`AllowRenewalIfOtherItemsAvailable` desligado);
   - cedo demais (`too_soon`): com `norenewalbefore`, a renovação só vale a partir do vencimento menos esse número de dias (ou de horas, se `lengthunit` é `hours`). Com `NoRenewalBeforePrecision` = `date` e empréstimo em dias, a partir do começo daquele dia; o padrão `exact_time` conta a hora exata. A recusa diz a data mais cedo.
3. Passar por cima dos limites dispensa a contagem, o atraso, o saldo devedor e a data mais cedo, mas não a reserva: no Koha, renovar um exemplar reservado pede outra permissão (`AllowRenewalOnHoldOverride`), que o Elzevir ainda não tem.
4. As regras e o calendário são os da unidade em que o exemplar está, da categoria do leitor e da política do exemplar, como no empréstimo ([emprestimo.md](emprestimo.md), item 13).
5. O novo vencimento parte do vencimento atual (`ManualRenewalPeriodBase` = `date_due`, o padrão) ou do momento da renovação (`now`), conforme `[circulation] renewal_base` do `elzevir.toml` (`due_date`, padrão, ou `now`; a troca do Koha traz `now` pela `import-koha-prefs`). Vale para o balcão, o OPAC, a API e o SIP2. O cálculo usa `renewalperiod` (ou `issuelength`, quando `renewalperiod` é vazio; ausente vale 0, como no Koha). Ele é truncado no minuto, como o Koha grava.
6. A renovação grava o novo vencimento, soma uma renovação e guarda o momento. O empréstimo encerrado leva a contagem.
7. Renovar pelo empréstimo (o exemplar já está com o mesmo leitor) ainda é recusado; a equipe usa a renovação. O balcão (3.6) junta os dois.

## 2. Origem no Koha

- `C4::Circulation::CanBookBeRenewed`, `GetSoonestRenewDate`, `AddRenewal`, `GetRenewCount` (renovações restantes).

## 3. Paridade

`exigida`. Prova, por enquanto: `crates/elzevir-domain/src/renewal.rs` (decisão pura) e `crates/elzevir-store/tests/checkout.rs` (renovação pelo comando: cedo demais, renovação a partir do vencimento e a partir do momento, contagem, passar por cima, sem empréstimo, contagem no empréstimo encerrado) e `crates/elzevir/tests/checkin_oracle.rs`, que reproduz `scenarios/circulacao/devolucao-renovacao.koha.json` (gravado por `tools/checkin-recorder/record.py`): cedo demais (`too_soon`), renovação com o calendário da unidade (nova data igual à do Koha) e limite de renovações (`too_many`). O saldo devedor e a reserva (`crates/elzevir-store/tests/checkout.rs`, teste de multa acima do limite e de reserva) não foram gravados no oráculo, que tem um só leitor e não deixa reservar para outra pessoa.

## 4. Casos de borda

1. `renewalsallowed` = `"many"`: lido como 0, nenhuma renovação.
2. `norenewalbefore` vazio: sem data mais cedo.
3. Renovar um empréstimo atrasado com `allow`: renova, e o novo vencimento parte do vencimento antigo; com `renewal_base = "now"`, parte do momento da renovação.

## 5. Desvios

Nenhum.
