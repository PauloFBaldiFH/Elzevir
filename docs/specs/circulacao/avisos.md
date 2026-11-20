# Avisos

> Status: tarefa 3.9, partes a (regras de aviso de atraso e fila de avisos), b (suspensão por aviso), c (envio por e-mail), d (aviso de reserva disponível), e (tela das regras de aviso), f (avisos impressos) e g (textos editáveis). Base: Koha 26.05.05. A parte seguinte traz o recibo.

## 1. Comportamento

### Regras de aviso de atraso

1. Uma regra de aviso de atraso vale para uma unidade (ou qualquer unidade) e uma categoria de leitor, e tem até três níveis. Cada nível tem um número de dias de atraso, os meios do aviso (e-mail, impresso, os dois ou nenhum) e se suspende o leitor.
2. Os dias dos níveis presentes sobem estritamente (o nível 2 depois do 1, o 3 depois do 2) e são positivos. Um nível pode faltar.
3. **Gravação**, comando `circ.set_overdue_notice_rules`, com a permissão de administração: recebe a unidade, a categoria e os níveis, e troca todos os níveis desse escopo; sem nenhum nível, a regra é apagada. Dias fora de ordem, nível repetido ou fora de 1 a 3, unidade ou categoria inexistente são recusados sem gravar nada.
4. Unidade e categoria de leitor usadas por uma regra de aviso não podem ser apagadas.

### Quais avisos saem

5. A corrida diária, comando `circ.queue_overdue_notices`, que o servidor tenta a cada hora junto com as multas, roda uma vez por dia, como o cron do Koha: a primeira tentativa do dia vale e as outras do mesmo dia não fazem nada. Ela olha os empréstimos cujo exemplar não está perdido e que venceram até hoje.
6. A unidade de um empréstimo é a do balcão onde ele foi feito (Koha: `OverdueNoticeFrom` = `cron`, que usa `issues.branchcode`).
7. **Regras da unidade.** Valem as regras da unidade do empréstimo. Quando a unidade não tem nenhuma regra de aviso, para nenhuma categoria, valem as de qualquer unidade. Uma unidade com regra só para outra categoria não cai nas de qualquer unidade: o leitor dessa categoria não recebe aviso.
8. Entre as regras que valem, a da categoria do leitor. Sem ela, nenhum aviso.
9. **Dias de atraso** são dias de calendário entre a data de vencimento e hoje, no fuso da biblioteca (Koha: `OverdueNoticeCalendar` desligado, o padrão). Com o calendário ligado, dias fechados da unidade não contam, e num dia em que a unidade está fechada não sai aviso nenhum dela.
10. **Disparo** (Koha roda `overdue_notices.pl -t` uma vez por dia): o nível sai no dia em que os dias de atraso de algum empréstimo do leitor naquela unidade são exatamente os dias do nível. O aviso lista os empréstimos do leitor naquela unidade com esse mesmo atraso.
11. Um leitor recebe no máximo um aviso por nível, unidade e dia; a corrida que roda de novo no mesmo dia não repete o aviso.
12. **Meios.** Cada meio do nível gera um aviso na fila. E-mail para um leitor sem e-mail vira impresso. Um leitor recebe no máximo um impresso por nível e dia, mesmo quando e-mail e impresso caem no impresso.
13. Cada aviso na fila guarda o leitor, o nível, a unidade, o meio, o endereço (o e-mail do leitor, no e-mail), o dia, a situação "pendente" e os exemplares com os vencimentos. O texto é montado no envio, pelos modelos (parte seguinte).
14. A resposta da corrida diz quantos avisos entraram na fila.

### Suspensão por aviso

15. Quando o nível que dispara suspende, o leitor recebe a suspensão "por atraso", sem fim (Koha: `AddUniqueDebarment` do tipo `OVERDUES`), mesmo sem nenhum meio de aviso. Um leitor já suspenso por atraso continua como está.
16. Essa suspensão se soma à suspensão por devolução atrasada ([suspensao.md](suspensao.md)): o leitor tem no máximo uma de cada tipo, e o empréstimo e a reserva são recusados enquanto qualquer uma vale; com a suspensão por atraso, a recusa é a sem fim (`USERBLOCKEDNOENDDATE`).
17. Ela só termina quando a equipe a retira (item 12 de [suspensao.md](suspensao.md)); devolver os exemplares não a retira (`AutoRemoveOverduesRestrictions` = `no`, o padrão). Como a corrida roda uma vez por dia, a suspensão retirada não volta no mesmo dia.

### Envio por e-mail

18. O servidor de e-mail fica em `[notices.email]` no `elzevir.toml`: remetente (`from`), servidor (`host`), porta (padrão 465 com `tls`, 587 com `starttls`, 25 com `none`), proteção (`security`, padrão `starttls`) e, quando o servidor pede, usuário (`user`) e o nome da variável de ambiente com a senha (`password_env`). A senha não fica no arquivo e nunca aparece em log. Sem a seção, nada é enviado e os avisos ficam pendentes.
19. A cada 15 minutos (o cron do Koha para `process_message_queue.pl`), o servidor envia até 100 avisos de e-mail pendentes, dos mais antigos aos mais novos.
20. O texto é escrito no envio, na língua do servidor: assunto pelo nível ("Exemplares em atraso", "Segundo aviso", "Último aviso") com o nome da unidade, e corpo com o nome do leitor, a unidade e uma linha por exemplar (título, código de barras, vencimento). A resposta vai para o e-mail da unidade, quando ela tem um.
21. Aviso aceito pelo servidor fica "enviado"; recusado de vez (endereço inválido, resposta 5xx) fica "falho" e não é tentado de novo. Servidor fora do ar ou recusa temporária deixam o aviso pendente, e o envio para até a próxima vez.
22. Avisos impressos continuam pendentes até a equipe imprimi-los (itens 31 a 33).

### Reserva disponível

23. Cada categoria de leitor diz como o leitor fica sabendo da reserva que o aguarda: e-mail, impresso, os dois ou nenhum (comando `circ.set_hold_notice`, com a permissão de administração; nenhum meio apaga a preferência). Sem preferência, não há aviso. No Koha é a preferência de mensagens `Hold_Filled` do leitor, que vem da categoria.
24. O aviso entra na fila quando a reserva passa a aguardar retirada ([reservas.md](reservas.md), item 12): separada no balcão da unidade de retirada, ou chegada nela depois do trânsito. A reserva em trânsito não gera aviso, e separar de novo uma reserva que já aguardava também não.
25. Os meios seguem o item 12: e-mail sem endereço vira impresso, no máximo um impresso.
26. O aviso guarda a unidade de retirada, o exemplar e o último dia para retirar; o texto diz o título, o código de barras, a unidade e até quando retirar. Ele sai pelo envio dos itens 18 a 21.
27. Uma categoria com preferência de aviso não pode ser apagada.

### Tela das regras de aviso

28. A tela "Avisos" (`/equipe/admin/avisos`, com a permissão de administração) lista as regras de aviso de atraso, as de qualquer unidade primeiro: unidade, categoria e, por nível, os dias, os meios e se suspende. Cada linha abre a regra; a escolha de unidade (ou qualquer) e categoria abre uma regra ainda vazia.
29. A regra aberta mostra os três níveis, cada um com os dias, e-mail, impresso e "suspende". Dias em branco tiram o nível; nenhum nível apaga a regra. Gravar manda o comando do item 3 com os níveis da tela; dias que não são um número inteiro positivo, ou a recusa do comando, voltam à tela com o motivo e o que foi digitado, sem gravar nada.
30. Na mesma tela, uma linha por categoria de leitor com e-mail e impresso para o aviso de reserva disponível. Gravar manda o comando do item 23 só para as categorias que mudaram.

### Avisos impressos

31. A tela "Avisos impressos" (`/equipe/circulacao/avisos`, com a permissão de circulação) mostra os avisos impressos pendentes, dos mais antigos aos mais novos, até 200: cada um com o nome e o endereço do leitor, o assunto e o texto do item 20, o mesmo do e-mail, na língua do servidor.
32. Impressa pelo navegador, a página tira o cabeçalho e os botões e põe um aviso por folha.
33. "Marcar como enviados" marca como enviados (`circ.mark_notice`, que passa a pedir a permissão de circulação) só os avisos que a página mostrou; um aviso que chegou depois continua pendente. Koha: `gather_print_notices.pl`, que junta os impressos pendentes num arquivo e os marca como enviados.

### Textos dos avisos

34. Há um texto (assunto e corpo) para cada aviso: primeiro, segundo e terceiro aviso de atraso e reserva disponível (Koha: as cartas `ODUE`, `ODUE2`, `ODUE3` e `HOLD`). Sem texto da biblioteca vale o texto padrão do item 20, na língua do servidor.
35. Na tela "Avisos", cada aviso mostra o texto da biblioteca ou, sem ele, o padrão com os marcadores no lugar dos dados. Gravar, com a permissão de administração, guarda o texto da biblioteca (comando `circ.set_notice_text`); gravar o texto igual ao padrão, ou o botão "Voltar ao texto padrão", apaga o da biblioteca.
36. Marcadores: `{name}` (o leitor), `{library}` (a unidade) e `{items}` (uma linha por exemplar, no formato do item 20) em todos; `{date}` (até quando retirar) só no aviso de reserva. O assunto é uma linha só, com até 200 caracteres; o corpo tem até 8.000. Texto vazio, assunto com mais de uma linha ou chave que não abre um marcador do aviso são recusados sem gravar nada, e a tela volta com o motivo e o que foi digitado.
37. O texto vale no envio, para o e-mail e para o impresso, também para os avisos que já estavam na fila; os dados entram como estão, sem que um marcador dentro deles seja preenchido de novo.

## 2. Origem no Koha

- `misc/cronjobs/overdue_notices.pl` com `-t` (`debian/koha-common.cron.daily`), `C4::Overdues::GetBranchcodesWithOverdueRules`, `GetOverdueMessageTransportTypes`.
- `tools/overduerules.pl`: dias numéricos, em ordem crescente; nível com dias e sem aviso nem suspensão é recusado. No Elzevir esse nível é aceito e não faz nada (item 29 grava o que a tela diz).
- Tabelas `overduerules` e `overduerules_transport_types`; `message_queue` (`C4::Letters::EnqueueLetter`).
- `C4::Reserves::ModReserveAffect` e `_koha_notify_reserve` (`Hold_Filled`, carta `HOLD`; impresso quando falta o e-mail).
- `misc/cronjobs/gather_print_notices.pl` (impressos).
- `tools/letter.pl` e as cartas `ODUE`, `ODUE2`, `ODUE3` e `HOLD` (textos).
- `misc/cronjobs/process_message_queue.pl` (`*/15` em `debian/koha-common.cron.d`), `C4::Letters::SendQueuedMessages` e `_set_message_status`.
- `Koha::Patron::Debarments::AddUniqueDebarment`, `UpdateBorrowerDebarmentFlags` (`9999-12-31` quando alguma restrição não tem fim), `DelUniqueDebarment`.
- `Koha::Calendar::days_between` (com `OverdueNoticeCalendar`), que tira os dias fechados contando as duas pontas.

## 3. Paridade

`exigida` nos dias de disparo. A corrida do Koha é um script de cron, que o gravador não alcança pela API; a prova é por testes contra o código lido: `crates/elzevir-domain/src/notice.rs` (dias com e sem calendário, nível que dispara, meios) e `crates/elzevir-store/tests/notices.rs` (regras da unidade e de qualquer unidade, categoria sem regra, atraso exato, um aviso por dia, e-mail que vira impresso, exemplar perdido fora, nível que suspende sem meio, empréstimo recusado até a equipe retirar, corrida repetida no dia que não suspende de novo, avisos de e-mail pendentes com seus exemplares, marcados como enviados). `crates/elzevir-store/tests/checkout.rs` testa o aviso de reserva: sem preferência nada sai, em trânsito nada sai, ao chegar sai um impresso para o leitor sem e-mail, e separar de novo não repete. A tela é testada em `crates/elzevir/tests/circulation_rules.rs` (gravar e ler uma regra, dias inválidos, preferência de reserva, sem permissão), e os impressos em `crates/elzevir/tests/patrons.rs` (listar, marcar como enviado). Os textos são testados em `crates/elzevir-domain/src/notice_text.rs` (marcadores e recusas), no envio (texto da biblioteca no lugar do padrão) e na tela (gravar, recusar, voltar ao padrão). O envio é testado contra um servidor SMTP falso em `crates/elzevir/src/notice_mail.rs` (aceito, recusado, fora do ar) e o texto nas duas línguas.

## 4. Casos de borda

1. Empréstimo com 3 dias de atraso e nível 1 em 3 dias: aviso hoje; amanhã, com 4 dias, não.
2. Dois empréstimos do leitor, um com 3 e outro com 5 dias de atraso, níveis em 3 e 5: dois avisos, um de cada nível, cada um com o seu exemplar.
3. A corrida roda duas vezes no dia: um aviso só.
4. Leitor sem e-mail num nível com e-mail e impresso: um impresso só.
5. Nível que suspende e sem meios: nenhum aviso na fila, mas o leitor fica suspenso.

## 5. Configuração

- No Koha, a categoria precisa de `overduenoticerequired` e de uma linha em `overduerules`. No Elzevir basta a regra: categoria sem aviso é categoria sem regra. Uma importação traduz `overduenoticerequired` desligado em nenhuma regra.
- O Koha escolhe um modelo (`letter1`..`letter3`) por nível e permite cartas por unidade e por língua, escritas em Template Toolkit; no Elzevir o texto é o do nível, um para a biblioteca, com os marcadores do item 36.
- O Koha guarda o texto pronto em `message_queue` ao enfileirar; o Elzevir escreve o texto no envio, com os dados guardados na fila.
- SMS e os outros meios do Koha vêm depois, com WhatsApp e Telegram (doc 03, linha 3.9).

## 6. Desvios

Nenhum.
