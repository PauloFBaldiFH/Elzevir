# Avisos

> Status: tarefa 3.9, parte a (regras de aviso de atraso e fila de avisos). Base: Koha 26.05.05. As partes seguintes trazem a suspensão por aviso, os modelos editáveis e o envio por SMTP, o aviso de reserva disponível e o recibo, e as telas.

## 1. Comportamento

### Regras de aviso de atraso

1. Uma regra de aviso de atraso vale para uma unidade (ou qualquer unidade) e uma categoria de leitor, e tem até três níveis. Cada nível tem um número de dias de atraso e os meios do aviso: e-mail, impresso, os dois ou nenhum.
2. Os dias dos níveis presentes sobem estritamente (o nível 2 depois do 1, o 3 depois do 2) e são positivos. Um nível pode faltar.
3. **Gravação**, comando `circ.set_overdue_notice_rules`, com a permissão de administração: recebe a unidade, a categoria e os níveis, e troca todos os níveis desse escopo; sem nenhum nível, a regra é apagada. Dias fora de ordem, nível repetido ou fora de 1 a 3, unidade ou categoria inexistente são recusados sem gravar nada.
4. Unidade e categoria de leitor usadas por uma regra de aviso não podem ser apagadas.

### Quais avisos saem

5. A corrida diária, comando `circ.queue_overdue_notices`, que o servidor roda a cada hora junto com as multas, olha os empréstimos cujo exemplar não está perdido e que venceram até hoje.
6. A unidade de um empréstimo é a do balcão onde ele foi feito (Koha: `OverdueNoticeFrom` = `cron`, que usa `issues.branchcode`).
7. **Regras da unidade.** Valem as regras da unidade do empréstimo. Quando a unidade não tem nenhuma regra de aviso, para nenhuma categoria, valem as de qualquer unidade. Uma unidade com regra só para outra categoria não cai nas de qualquer unidade: o leitor dessa categoria não recebe aviso.
8. Entre as regras que valem, a da categoria do leitor. Sem ela, nenhum aviso.
9. **Dias de atraso** são dias de calendário entre a data de vencimento e hoje, no fuso da biblioteca (Koha: `OverdueNoticeCalendar` desligado, o padrão). Com o calendário ligado, dias fechados da unidade não contam, e num dia em que a unidade está fechada não sai aviso nenhum dela.
10. **Disparo** (Koha roda `overdue_notices.pl -t` uma vez por dia): o nível sai no dia em que os dias de atraso de algum empréstimo do leitor naquela unidade são exatamente os dias do nível. O aviso lista os empréstimos do leitor naquela unidade com esse mesmo atraso.
11. Um leitor recebe no máximo um aviso por nível, unidade e dia; a corrida que roda de novo no mesmo dia não repete o aviso.
12. **Meios.** Cada meio do nível gera um aviso na fila. E-mail para um leitor sem e-mail vira impresso. Um leitor recebe no máximo um impresso por nível e dia, mesmo quando e-mail e impresso caem no impresso.
13. Cada aviso na fila guarda o leitor, o nível, a unidade, o meio, o endereço (o e-mail do leitor, no e-mail), o dia, a situação "pendente" e os exemplares com os vencimentos. O texto é montado no envio, pelos modelos (parte seguinte).
14. A resposta da corrida diz quantos avisos entraram na fila.

## 2. Origem no Koha

- `misc/cronjobs/overdue_notices.pl` com `-t` (`debian/koha-common.cron.daily`), `C4::Overdues::GetBranchcodesWithOverdueRules`, `GetOverdueMessageTransportTypes`.
- `tools/overduerules.pl`: dias numéricos, em ordem crescente; nível com dias e sem aviso nem suspensão é recusado.
- Tabelas `overduerules` e `overduerules_transport_types`; `message_queue` (`C4::Letters::EnqueueLetter`).
- `Koha::Calendar::days_between` (com `OverdueNoticeCalendar`), que tira os dias fechados contando as duas pontas.

## 3. Paridade

`exigida` nos dias de disparo. A corrida do Koha é um script de cron, que o gravador não alcança pela API; a prova é por testes contra o código lido: `crates/elzevir-domain/src/notice.rs` (dias com e sem calendário, nível que dispara, meios) e `crates/elzevir-store/tests/notices.rs` (regras da unidade e de qualquer unidade, categoria sem regra, atraso exato, um aviso por dia, e-mail que vira impresso, exemplar perdido fora).

## 4. Casos de borda

1. Empréstimo com 3 dias de atraso e nível 1 em 3 dias: aviso hoje; amanhã, com 4 dias, não.
2. Dois empréstimos do leitor, um com 3 e outro com 5 dias de atraso, níveis em 3 e 5: dois avisos, um de cada nível, cada um com o seu exemplar.
3. A corrida roda duas vezes no dia: um aviso só.
4. Leitor sem e-mail num nível com e-mail e impresso: um impresso só.

## 5. Configuração

- No Koha, a categoria precisa de `overduenoticerequired` e de uma linha em `overduerules`. No Elzevir basta a regra: categoria sem aviso é categoria sem regra. Uma importação traduz `overduenoticerequired` desligado em nenhuma regra.
- O Koha escolhe um modelo (`letter1`..`letter3`) por nível; no Elzevir o modelo é o do nível (parte dos modelos).
- SMS e os outros meios do Koha vêm depois, com WhatsApp e Telegram (doc 03, linha 3.9).

## 6. Desvios

Nenhum.
