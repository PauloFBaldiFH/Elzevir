# Desvios conhecidos

Diferenças **intencionais** entre o Elzevir e o Koha 26.05.05 nas áreas de paridade exigida (datas e calendário, multas, fila de reservas, exportação MARC, SIP2, Z39.50; ver [04-decisoes.md](arquitetura/04-decisoes.md), §2).

Regra: o Elzevir é exato por padrão. Um cenário diferencial só pode divergir do oráculo do Koha se o desvio estiver listado aqui. O executor de cenários (tarefa 1.0) lê este arquivo, aceita a divergência de um cenário listado e rejeita qualquer outra; `cargo xtask check-deviations` (no CI) valida o formato.

## Formato

Cada desvio é uma seção `## DEV-NNNN: título` com **um** bloco `deviation` de chaves simples, seguido do motivo em prosa:

````text
## DEV-0001: Vencimento não cai em feriado cadastrado depois do empréstimo

```deviation
id: DEV-0001
area: circulacao
koha: C4::Circulation::CalcDateDue
scenarios: scenarios/circulacao/vencimento-feriado-posterior.yaml
decided_by: Paulo
date: 2026-11-12
```

Por que divergimos, o que o Koha faz, o que o Elzevir faz e como isso aparece para a biblioteca.
````

| Chave | Obrigatória | Conteúdo |
|---|---|---|
| `id` | sim | `DEV-` + 4 dígitos, igual ao título, em ordem crescente |
| `area` | sim | `circulacao`, `multas`, `reservas`, `marc`, `sip2` ou `z3950` |
| `koha` | sim | Sub(s) do Koha afetadas, separadas por vírgula |
| `scenarios` | sim | Cenário(s) que divergem, separados por vírgula |
| `decided_by` | sim | Quem aprovou |
| `date` | sim | `AAAA-MM-DD` |

## Desvios

## DEV-0001: Apagar um fechamento semanal ou anual mantém as exceções abertas

```deviation
id: DEV-0001
area: circulacao
koha: C4::Calendar::delete_holiday
scenarios: crates/elzevir-store/tests/calendar.rs
decided_by: Paulo
date: 2026-11-19
```

O Koha, ao apagar um fechamento semanal, marca como fechadas (`isexception = 0`) as exceções da unidade cujo `WEEKDAY()` do MySQL é igual ao dia guardado. Como o `WEEKDAY()` conta a partir de segunda-feira e o dia guardado a partir de domingo, as datas atingidas são as do dia seguinte. Ao apagar um fechamento anual, faz o mesmo com as exceções daquele dia e mês. Nos dois casos, datas que a biblioteca abriu de propósito passam a fechar. O Elzevir apaga só o fechamento e deixa as exceções como estão. O cálculo do vencimento e da multa com um calendário dado é igual ao do Koha; a diferença aparece só depois dessa edição ([calendario.md](specs/circulacao/calendario.md), §5).

## DEV-0002: Um fechamento novo num dia já fechado troca o título

```deviation
id: DEV-0002
area: circulacao
koha: C4::Calendar::insert_single_holiday, C4::Calendar::insert_week_day_holiday, C4::Calendar::insert_day_month_holiday
scenarios: crates/elzevir-store/tests/calendar.rs
decided_by: Paulo
date: 2026-11-19
```

O `tools/newHolidays.pl` do Koha não grava uma entrada num dia que já é feriado (`isHoliday`). O Elzevir grava e troca o título da entrada do mesmo tipo. Os dias fechados são os mesmos; muda só o nome mostrado ([calendario.md](specs/circulacao/calendario.md), §5).

## DEV-0003: O servidor SIP2 v0 responde `96` às mensagens que não trata

```deviation
id: DEV-0003
area: sip2
koha: C4::SIP::Sip::MsgType::handle_block_patron, C4::SIP::Sip::MsgType::handle_hold, C4::SIP::Sip::MsgType::handle_item_status_update, C4::SIP::Sip::MsgType::handle_patron_enable, C4::SIP::Sip::MsgType::handle_fee_paid, C4::SIP::Sip::MsgType::handle_renew_all
scenarios: crates/elzevir-sip2/tests/codec.rs, crates/elzevir/tests/sip2_oracle.rs
decided_by: Paulo
date: 2026-11-26
```

O Koha trata bloqueio do leitor (`01`), reserva (`15`), atualização do exemplar (`19`), habilitação do leitor (`25`), pagamento (`37`) e renovar tudo (`65`), e aceita transações guardadas offline (`BI` e o `98` com `offline_ok`). O Elzevir v0 trata login, estado do terminal, leitor, exemplar, empréstimo, devolução, renovação e fim de sessão; às outras responde `96`, não as anuncia no `BX` e diz `N` em `offline_ok`. Um terminal que segue o `BX` não as envia ([sip2.md](specs/circulacao/sip2.md), itens 13 e 29).

## DEV-0004: Saudação e unidades do SIP2 saem com o nome do Elzevir e da unidade

```deviation
id: DEV-0004
area: sip2
koha: C4::SIP::ILS::Patron::new, C4::SIP::ILS::Item::new
scenarios: crates/elzevir/tests/sip2_server.rs, crates/elzevir/tests/sip2_oracle.rs
decided_by: Paulo
date: 2026-11-26
```

O Koha começa a mensagem de tela do leitor com `Greetings from Koha. ` escreve as unidades (`AO`, `AQ`, `AP`, `BG`, `CT`) pelo código do `branchcode` e a categoria do leitor (`PC`) pelo `categorycode`. O Elzevir escreve `Greetings from Elzevir. `, as unidades e a categoria pelo nome, porque o núcleo não tem códigos de unidade nem de categoria ([fronteira arquitetural](../CLAUDE.md)). Um terminal configurado com o código da unidade do Koha passa a usar o nome cadastrado no Elzevir ([sip2.md](specs/circulacao/sip2.md), itens 16 e 30).

## DEV-0005: O SIP2 não cobra taxa de empréstimo nem de renovação

```deviation
id: DEV-0005
area: sip2
koha: C4::SIP::ILS::Transaction::Checkout::do_checkout, C4::SIP::ILS::Transaction::Renew::do_renew
scenarios: crates/elzevir/tests/sip2_server.rs
decided_by: Paulo
date: 2026-11-26
```

O Koha informa no `12` e no `30` a taxa de aluguel do tipo de item (`BV`, `BH`, `BT`). O Elzevir não tem tipos de item nem taxas de empréstimo, então o `BV` dessas respostas não sai ([sip2.md](specs/circulacao/sip2.md), item 30).
