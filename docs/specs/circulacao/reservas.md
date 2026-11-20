# Reservas

> Status: tarefa 3.8, parte a (pedir, fila e cancelar, pela ficha do leitor). Atender a reserva na devolução, a retirada pelo leitor, o fim da espera e a disponibilidade no OPAC vêm nas próximas partes. Base: Koha 26.05.05.

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
10. A ficha do leitor lista as reservas, da mais antiga à mais nova, com a posição na fila, o título, o exemplar (reserva de exemplar), a unidade de retirada e o dia.
11. A recusa mostra o motivo. Uma reserva feita mostra a posição na fila.

## 2. Origem no Koha

- `Koha::REST::V1::Holds::add`, `Koha::Patron::can_place_holds`.
- `C4::Reserves::CanBookBeReserved`, `CanItemBeReserved`, `CalculatePriority`, `AddReserve`, `FixPriority`.
- `Koha::Item::pickup_locations`, `Koha::Biblio::pickup_locations`, `C4::Circulation::GetBranchItemRule`.
- `Koha::Hold::cancel`.
- Testes: `reference/koha/t/db_dependent/Reserves.t`, `Holds.t`.

## 3. Paridade

`exigida`. Prova: `crates/elzevir-domain/src/hold.rs` (cada recusa do item 4, unidades de retirada), `crates/elzevir-store/tests/checkout.rs` (fila, posições depois do cancelamento, dívida) e `crates/elzevir/tests/patrons.rs` (pedir e cancelar pela ficha). `crates/elzevir/tests/holds_oracle.rs` reproduz `scenarios/circulacao/reservas.koha.json` (gravado por `tools/holds-recorder/record.py` pela API REST de reservas do Koha): sem reservas permitidas, reserva de registro, segunda reserva no mesmo registro, reserva de exemplar repetida, limite por dia, limite total, cancelamento e a fila que sobra dão as mesmas respostas e posições que o Koha. A unidade de retirada não entra na gravação, porque o oráculo tem uma unidade só.

## 4. Casos de borda

1. Instância sem exemplares: a reserva de registro é recusada pela unidade de retirada, porque nenhum exemplar a serve.
2. Dívida igual a `maxoutstanding`: não recusa (o Koha compara com "maior que").
3. Crédito não usado diminui a dívida do item 3.3, ao contrário do empréstimo, que só olha os débitos.
4. Duas reservas de registro do mesmo leitor na mesma Instância: a segunda é recusada pelo padrão de `holds_per_record`.

## 5. Configuração

`maxoutstanding`, `maxreserves`, `ReservesControlBranch`, `ReservesNeedReturns`, `AllowHoldsOnDamagedItems` e `AllowHoldsOnPatronsPossessions` seguem os padrões do Koha até chegarem ao `elzevir.toml`. As regras dos itens 4 e 5 são regras de circulação comuns.

## 6. Fora desta parte

- Reserva com data futura (`AllowHoldDateInFuture`), data de expiração pedida pelo leitor, observações, reserva suspensa, prioridade mínima e mudança manual de posição.
- Reserva por tipo de material e por grupo de exemplares; grupos de unidades; limites de transferência entre unidades.
- Taxa de reserva (`HoldFeeMode`); restrição de idade; reservas de leitores pela internet.
- Passar por cima das regras (`AllowHoldPolicyOverride`).
