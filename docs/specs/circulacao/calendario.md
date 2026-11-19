# Calendário das unidades

> Status: tarefa 3.3, partes a (gravação e leitura) e b (tela e cópia). Base: Koha 26.05.05. O cálculo com o calendário (dia fechado, busca de dia aberto) está em [vencimento.md](vencimento.md), itens 11 e 12.

## 1. Comportamento

1. Cada unidade tem o seu calendário. Uma unidade sem nenhuma entrada fica aberta todos os dias.
2. Há três tipos de entrada, cada uma com um título (1 a 50 caracteres, sem espaços nas pontas) e uma descrição opcional (até 2.000 caracteres):
   - **semanal**: fechado toda semana num dia da semana (0 = domingo, como no Koha);
   - **anual**: fechado todo ano num dia e mês (29 de fevereiro vale; 31 de abril é recusado);
   - **avulsa**: uma data fechada, ou aberta como exceção a uma entrada semanal ou anual.
3. O comando `circ.change_calendar`, com a permissão de administração (Koha: `tools.edit_calendar`), faz uma destas coisas numa unidade:
   - fecha um dia da semana;
   - fecha um dia do ano;
   - marca um intervalo de datas, do primeiro ao último dia, como fechado ou como exceção aberta (um dia só é o intervalo de um dia; no máximo 366 dias);
   - reabre um dia da semana, tira um dia do ano ou tira as datas avulsas de um intervalo;
   - copia o calendário para outra unidade (Koha: `C4::Calendar::copy_to_branch`): a outra unidade recebe cada entrada semanal e anual e cada data avulsa depois de hoje que ela ainda não tem; o que ela já tem fica como está.
4. Gravar uma entrada que já existe troca o título e, nas avulsas, fechado por aberto ou o contrário. Cada dia tem no máximo uma entrada de cada tipo.
5. São recusados, sem gravar nada: unidade inexistente, dia da semana depois de sábado, título vazio ou longo, dia do ano que não existe, intervalo de trás para frente ou maior que 366 dias.
6. A leitura devolve o calendário do domínio (`Calendar`) para o vencimento e as multas, e a lista das entradas com título para a tela: semanais, anuais e avulsas, cada grupo em ordem.
7. Apagar uma unidade apaga o calendário dela, como no Koha.
8. O calendário não entra no resumo da importação.

### Tela (`/equipe/admin/calendario`)

9. Pede a permissão de administração, a mesma do comando. Sem ela, 403.
10. Escolhe-se a unidade e o mês (o mês de hoje no fuso da biblioteca, por padrão), com setas para o mês anterior e o seguinte.
11. A grade do mês começa no domingo. Cada dia mostra se está fechado ou aberto por exceção, com o título da entrada que decide o dia: a avulsa, senão a semanal, senão a anual. O estado vem de `Calendar::is_closed`, a mesma função do vencimento.
12. Três formulários acrescentam: datas (de, até, aberto por exceção), dia da semana e dia do ano, cada um com título e descrição. "Até" vazio é o mesmo dia de "de".
13. As entradas aparecem em três listas (semanais, anuais e avulsas do mês mostrado em diante), cada uma com o botão de tirar.
14. Com mais de uma unidade, um formulário copia o calendário para outra. Uma recusa mostra a mensagem do Fluent e a tela de novo, sem gravar nada.

## 2. Origem no Koha

- Tabelas `repeatable_holidays` (semanal com `weekday`, anual com `day` e `month`) e `special_holidays` (data com `isexception`).
- `C4::Calendar::insert_week_day_holiday`, `insert_day_month_holiday`, `insert_single_holiday`, `insert_exception_holiday`, `delete_holiday`, `delete_holiday_range`.
- `tools/newHolidays.pl` (intervalos `holidayrange` e `holidayrangerepeat`) e `tools/exceptionHolidays.pl`.
- `Koha::Calendar::is_holiday` lê as três: a data avulsa vence, depois a semanal, depois a anual.

## 3. Paridade

`exigida` no efeito do calendário sobre vencimento e multa. Prova:

- `crates/elzevir/tests/calendar.rs`: cada caso de `scenarios/circulacao/calcdatedue.t.yaml` e `calcfine.t.yaml` roda com o calendário gravado numa unidade pelo comando e relido do banco, e o calendário relido é igual ao original.
- `crates/elzevir-store/tests/calendar.rs`: gravar, trocar o título, exceção, tirar intervalo, reabrir dia da semana, tirar dia do ano, unidade apagada, recusas sem gravar nada, cópia que guarda o que a outra unidade já tinha.
- `crates/elzevir/tests/calendar_screen.rs`, contra o servidor real: permissão, acrescentar e tirar pela tela, grade do mês, cópia.

## 4. Casos de borda

1. Exceção aberta num sábado fechado toda semana: o sábado conta como aberto.
2. Intervalo que já tinha uma data avulsa: a data fica com o título novo.
3. Tirar um intervalo sem datas avulsas: nada muda, e o comando diz que tirou zero.
4. 29 de fevereiro anual: fecha só nos anos bissextos.

## 5. Desvios

Na edição, não no cálculo, aprovados por Paulo em 2026-11-19 (DEV-0001 e DEV-0002 em [desvios-conhecidos.md](../../desvios-conhecidos.md)):

1. O Koha não grava uma entrada num dia que já é feriado (`isHoliday`); o Elzevir troca o título da entrada daquele tipo. O efeito no cálculo é o mesmo.
2. Ao apagar uma entrada semanal, o Koha transforma as exceções da unidade em datas fechadas, e escolhe as datas por `WEEKDAY()` do MySQL, que conta a partir de segunda-feira, enquanto o dia guardado conta a partir de domingo. Ao apagar uma entrada anual, faz o mesmo com as exceções daquele dia e mês. Isso fecha datas que a biblioteca tinha aberto. O Elzevir deixa as exceções como estão.
