# Reservas

> Status: tarefa 3.8, partes a (pedir, fila e cancelar, pela ficha do leitor), b (a devolução encontra a reserva e a equipe separa o exemplar) c (o empréstimo pergunta antes de levar o exemplar de outro leitor e atende a reserva de quem o leva) e d (a reserva que passou do prazo de retirada é cancelada). A disponibilidade no OPAC vem na próxima parte. Base: Koha 26.05.05.

## 1. Comportamento

1. Uma reserva é o pedido de um leitor por uma Instância (qualquer exemplar dela, "reserva de registro") ou por um exemplar dela ("reserva de exemplar"), para retirar numa unidade escolhida.
2. As reservas de uma Instância formam uma fila. Cada reserva ainda não atendida tem uma posição, 1, 2, 3..., sem buracos. Uma reserva nova entra no fim da fila.
3. **Pedir.** Na ficha do leitor, com a permissão de circulação, a equipe digita o código de barras de um exemplar, escolhe "este exemplar" ou "qualquer exemplar do registro" e a unidade de retirada. A reserva é recusada, nesta ordem (a ordem do `POST /api/v1/holds` do Koha):
   1. a unidade de retirada não serve para o exemplar (reserva de exemplar) ou para nenhum exemplar da Instância (reserva de registro), pelo item 5;
   2. o leitor está suspenso (`restricted`);
   3. o leitor deve mais que `maxoutstanding` (padrão 5,00), contando débitos e créditos (`debt_limit`);
   4. o leitor já tem `maxreserves` reservas (padrão 50; `hold_limit`);
   5. as regras de circulação não permitem a reserva (item 4); na reserva de registro, basta um exemplar da Instância permitir, e a recusa mostrada é a do último exemplar.
4. **Regras de um exemplar** (Koha: `CanItemBeReserved`), olhadas com a unidade do leitor (`ReservesControlBranch` = `PatronLibrary`), a categoria do leitor e a política do exemplar, nesta ordem:
   1. o leitor já tem uma reserva desse exemplar: recusa (`itemAlreadyOnHold`);
   2. `holds_per_record` (padrão 1 quando não há regra): zero recusa (`noReservesAllowed`); o leitor já com essa quantidade de reservas na Instância recusa (`tooManyHoldsForThisRecord`);
   3. `holds_per_day`, quando preenchida: o leitor já com essa quantidade de reservas feitas hoje recusa (`tooManyReservesToday`);
   4. `reservesallowed`: sem regra, ou com valor zero, recusa (`noReservesAllowed`); vazia, não limita; com número, conta as reservas do leitor de exemplares com a política da regra (ou todas, quando a regra vale para qualquer política) e recusa quando a conta chega ao número (`tooManyReserves`). Reservas de registro não têm política e só contam na regra de qualquer política;
   5. `max_holds` (por unidade e categoria), quando preenchida: o leitor já com essa quantidade de reservas recusa (`tooManyReserves`);
   6. `holdallowed` (por unidade e política; padrão "de qualquer unidade"): "não permitido" recusa (`notReservable`); "só da unidade do exemplar" recusa quando a unidade do leitor não é a do exemplar (`cannotReserveFromOtherBranches`); "do grupo local" faz o mesmo, porque o Elzevir ainda não tem grupos de unidades (`branchNotInHoldGroup`).
5. **Unidades de retirada** (Koha: `Koha::Item::pickup_locations`): nenhuma quando o item 4.6 recusaria; senão, pela regra `hold_fulfillment_policy` (padrão "qualquer"): qualquer unidade; só a unidade do exemplar; só a unidade onde ele está; nenhuma para "grupo de reservas" e "grupo do leitor", que dependem de grupos que ainda não existem.
6. A situação do exemplar não é olhada ao pedir: emprestado, perdido ou indisponível, ele pode ser reservado, como no Koha com `AllowHoldsOnDamagedItems` e `AllowHoldsOnPatronsPossessions` ligados (os padrões). O exemplar que o próprio leitor tem emprestado também.
7. A reserva guarda o dia em que foi feita, a unidade de retirada e, na reserva de exemplar, o exemplar. Ela não fica pronta para retirada ao ser feita, mesmo com o exemplar na estante (`ReservesNeedReturns` ligado, o padrão).
8. **Cancelar.** A equipe cancela uma reserva na ficha do leitor. A reserva sai da fila e vai para o histórico de reservas com o dia do cancelamento; as posições de quem estava atrás sobem.
9. O histórico guarda o leitor só quando o leitor mantém histórico de leitura, como os empréstimos devolvidos.
10. A ficha do leitor lista as reservas, da mais antiga à mais nova, com a posição na fila (ou "aguarda retirada até D", ou "em trânsito"), o título, o exemplar (reserva de exemplar ou reserva já separada), a unidade de retirada e o dia.
11. A recusa mostra o motivo. Uma reserva feita mostra a posição na fila.
12. **Separada.** Uma reserva separada para o leitor sai da fila (posição 0) e fica com um exemplar: "aguardando" na unidade de retirada, ou "em trânsito" para ela. A fila das outras se renumera 1, 2, 3...; uma reserva nova entra depois só das que não foram separadas.
13. **Qual reserva o exemplar atende** (Koha: `CheckReserves`). Exemplar com situação "não emprestável" positiva não atende nenhuma. Entre as reservas da Instância que são de registro ou deste exemplar: primeiro a reserva já separada com ele (aguardando ou em trânsito); senão, a de menor posição que as regras deixam o exemplar atender, olhadas com a unidade do leitor da reserva e a política do exemplar: `holdallowed` "não permitido" não deixa; "só da unidade do exemplar" só deixa quando a unidade do leitor é a do exemplar; "do grupo local" não deixa, por não haver grupos. `hold_fulfillment_policy` "unidade do exemplar" exige retirada na unidade do exemplar; "unidade onde está" exige retirada onde ele está (a da devolução); "grupo" não deixa.
14. **Na devolução** (Koha: `AddReturn`), o exemplar não baixado procura a reserva pelo item 13, já na unidade da devolução. Se a encontra:
    1. a reserva que aguarda em outra unidade volta para a fila na posição 1 (as outras descem uma), deixa de aguardar e esquece o exemplar, a não ser que seja reserva de exemplar; a devolução a trata como reserva ainda não separada;
    2. o exemplar não é mandado de volta à unidade de origem (item 7 de `devolucao.md`); a transferência que chega continua chegando;
    3. a tela de devolução diz que o exemplar atende a uma reserva e onde ela é retirada, sem dados do leitor, e oferece o botão "Separar para a reserva"; a reserva que já aguarda ali só é avisada.
15. **Separar** (Koha: `ModReserveAffect`, confirmado na tela de devolução). O exemplar precisa ser da Instância da reserva e, na reserva de exemplar, o próprio. Se a retirada é na unidade da devolução, a reserva passa a aguardar: guarda o dia em que começou a aguardar (o que já tinha, se já aguardava) e o último dia para retirar, esse dia mais `holds_pickup_period` (unidade de retirada, categoria do leitor, política do exemplar; sem regra, 7 dias de `ReservesMaxPickUpDelay`), em dias corridos; a transferência aberta do exemplar chega. Senão, a reserva fica em trânsito e o exemplar é mandado da unidade da devolução para a de retirada, com motivo `Reserve`, no lugar da transferência aberta. A tela diz o último dia ou para onde mandar.
16. **No empréstimo, reserva de outro leitor** (Koha: `CanBookBeIssued`). O exemplar procura a reserva pelo item 13, na unidade onde está antes do empréstimo. Se ela é de outro leitor, o empréstimo pede confirmação, depois das outras: o exemplar aguarda a retirada dessa reserva (`RESERVE_WAITING`), está em trânsito para ela (`TRANSFERRED`) ou pode atendê-la (`RESERVED`). A mensagem não mostra quem é o outro leitor. Confirmado, o empréstimo acontece e a reserva do outro continua como estava.
17. **No empréstimo, a reserva de quem leva** (Koha: `MoveReserve`, `Koha::Hold::fill`). Depois do empréstimo, a reserva atendida é a que o item 16 achou, quando é do próprio leitor; senão, a primeira reserva do leitor na Instância (pela posição; as separadas, com posição 0, primeiro) que seja de registro ou deste exemplar. Ela é atendida quando já está com este exemplar; com outro exemplar ou nenhum, só quando a regra `fill_other_biblio_holds_policy` (unidade do leitor, política do exemplar; sem regra, sim) não é falsa. A reserva atendida vai para o histórico como `FILLED`, com o exemplar emprestado, e a fila se renumera.
18. **Prazo vencido** (Koha: `CancelExpiredReserves`, `misc/cronjobs/holds/cancel_expired_holds.pl`). Com `expire_waiting = true` em `[circulation.holds]` do `elzevir.toml` (padrão desligado, como `ExpireReservesMaxPickUpDelay`), o servidor cancela, ao subir e depois de hora em hora, cada reserva que aguarda e cujo último dia para retirar é anterior a hoje (no fuso da biblioteca). Ela vai para o histórico como cancelada, com o exemplar, e a fila se renumera. Dia fechado não adia o cancelamento (`ExpireReservesOnHolidays` ligada, o padrão). O exemplar fica onde está; a próxima devolução dele procura a próxima reserva (`ExpireReservesAutoFill` desligada). Rodar de hora em hora cancela o mesmo que rodar uma vez por dia, porque só olha o dia.

## 2. Origem no Koha

- `Koha::REST::V1::Holds::add`, `Koha::Patron::can_place_holds`.
- `C4::Reserves::CanBookBeReserved`, `CanItemBeReserved`, `CalculatePriority`, `AddReserve`, `FixPriority`.
- `Koha::Item::pickup_locations`, `Koha::Biblio::pickup_locations`, `C4::Circulation::GetBranchItemRule`.
- `Koha::Hold::cancel`.
- `C4::Reserves::CancelExpiredReserves`, `Koha::Hold::cancel`.
- `C4::Reserves::CheckReserves`, `ModReserveAffect`, `C4::Circulation::AddReturn`; `Koha::Hold::set_waiting`, `set_transfer`, `revert_found`; `circ/returns.pl`.
- Testes: `reference/koha/t/db_dependent/Reserves.t`, `Holds.t`.

## 3. Paridade

`exigida`. Prova: `crates/elzevir-domain/src/hold.rs` (cada recusa do item 4, unidades de retirada), `crates/elzevir-store/tests/checkout.rs` (fila, posições depois do cancelamento, dívida) e `crates/elzevir/tests/patrons.rs` (pedir e cancelar pela ficha; separar na devolução). A parte b tem prova em `crates/elzevir-domain/src/hold.rs` (item 13) e `crates/elzevir-store/tests/checkout.rs` (itens 14 e 15: separar aguardando, voltar à fila, trânsito e chegada; itens 16 e 17: confirmação, atender a própria reserva, a regra `fill_other_biblio_holds_policy`; item 18: o último dia ainda espera, o dia seguinte cancela) e `crates/elzevir/src/config.rs` (a chave `expire_waiting`). O item 18 não tem gravação: o cancelamento do Koha roda por cron no servidor, fora do alcance do gravador pela web. `crates/elzevir/tests/holds_aside_oracle.rs` reproduz `scenarios/circulacao/reservas-devolucao.koha.json` (gravado por `tools/holds-recorder/set_aside.py` com `circ/returns.pl`): a devolução encontra a reserva de registro, e separar a deixa aguardando, na posição 0, com o exemplar e o mesmo último dia que o Koha, com `holds_pickup_period` vazio (7 dias) e com 3; depois o próprio leitor leva o exemplar sem confirmação de reserva e a reserva é atendida. A confirmação para outro leitor não entra na gravação, porque o oráculo tem um leitor de exemplo só. O trânsito para outra unidade não entra na gravação, porque o oráculo tem uma unidade só. `crates/elzevir/tests/holds_oracle.rs` reproduz `scenarios/circulacao/reservas.koha.json` (gravado por `tools/holds-recorder/record.py` pela API REST de reservas do Koha): sem reservas permitidas, reserva de registro, segunda reserva no mesmo registro, reserva de exemplar repetida, limite por dia, limite total, cancelamento e a fila que sobra dão as mesmas respostas e posições que o Koha. A unidade de retirada não entra na gravação, porque o oráculo tem uma unidade só.

## 4. Casos de borda

1. Instância sem exemplares: a reserva de registro é recusada pela unidade de retirada, porque nenhum exemplar a serve.
2. Dívida igual a `maxoutstanding`: não recusa (o Koha compara com "maior que").
3. Crédito não usado diminui a dívida do item 3.3, ao contrário do empréstimo, que só olha os débitos.
4. Duas reservas de registro do mesmo leitor na mesma Instância: a segunda é recusada pelo padrão de `holds_per_record`.
5. Exemplar de reserva em trânsito devolvido na unidade de retirada: a transferência chega e a devolução aponta a mesma reserva; separar a faz aguardar.
6. Separar de novo uma reserva que já aguarda ali mantém o dia em que começou a aguardar.
7. Exemplar baixado: a devolução é recusada antes (`BlockReturnOfWithdrawnItems`); mesmo sem o bloqueio, ele não procura reserva.

## 5. Configuração

`maxoutstanding`, `maxreserves`, `ReservesControlBranch`, `ReservesNeedReturns`, `AllowHoldsOnDamagedItems`, `AllowHoldsOnPatronsPossessions`, `ReservesMaxPickUpDelay` (7), `ExcludeHolidaysFromMaxPickUpDelay` (desligada), `ConfirmFutureHolds` (0), `LocalHoldsPriority` (nenhuma), `TrapHoldsOnOrder` (ligada), `HoldsAutoFill` (desligada: a equipe confirma), `ExpireReservesOnHolidays` (ligada), `ExpireReservesAutoFill` (desligada) e `ExpireReservesMaxPickUpDelayCharge` (0) seguem os padrões do Koha até chegarem ao `elzevir.toml`. As regras dos itens 4 e 5 são regras de circulação comuns. `ExpireReservesMaxPickUpDelay` é `expire_waiting` em `[circulation.holds]` (item 18).

## 6. Fora desta parte

- Reserva com data futura (`AllowHoldDateInFuture`), data de expiração pedida pelo leitor, observações, reserva suspensa, prioridade mínima e mudança manual de posição.
- Reserva por tipo de material e por grupo de exemplares; grupos de unidades; limites de transferência entre unidades.
- Taxa de reserva (`HoldFeeMode`); restrição de idade; reservas de leitores pela internet.
- Passar por cima das regras (`AllowHoldPolicyOverride`).
- Cancelar ou devolver à fila a reserva do outro leitor ao confirmar o empréstimo (`cancelreserve`); taxa de reserva cobrada ao atender; cobrança da reserva vencida (`expire_reserves_charge`); vencimento da reserva pedido pelo leitor; vencimento adiado por dia fechado.
- Fila de reservas calculada (`RealTimeHoldsQueue`), lista de exemplares a buscar, mesa de retirada (`desk_id`) e avisos ao leitor (tarefa 3.9).
