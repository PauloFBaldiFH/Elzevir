# Conta do leitor no OPAC

> Status: tarefa 3.10, partes a a d. Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 3, linha 3.10) e [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §23 item 6. As partes são: entrada (a), empréstimos e débitos (b), renovação (c), reservas e histórico de leitura (d).

## 1. Comportamento

### Senha do leitor (parte a)

1. **Sem senha, sem conta.** Um leitor só entra no OPAC depois que a equipe define uma senha para ele. O leitor nasce sem senha.
2. **Quem define.** A senha é definida na edição do leitor, por quem tem a permissão de leitores, pelo comando `patrons.set_password`. Uma senha nova substitui a anterior. Também dá para tirar a senha, e aí o leitor não entra mais.
3. **Regra da senha.** A senha tem de 8 a 128 caracteres. Não pode ser igual à carteirinha nem ao CPF.
4. **Guarda.** O servidor guarda só o hash Argon2id, nunca a senha. O comando que vai para o `audit_log` leva só o hash.

### Entrada (parte a)

5. **Como entra.** O leitor entra pelo número da carteirinha e pela senha, no painel "Conta do leitor" do início do OPAC ou em `GET /conta/entrar`. O formulário vai para `POST /conta/entrar` e nunca pede o nome.
6. **Recusa igual.** Carteirinha desconhecida, leitor sem senha e senha errada dão a mesma recusa, "Carteirinha ou senha incorretas". O tempo de resposta também é o mesmo, porque o servidor confere um hash de mentira quando não há hash de verdade. Assim, o OPAC não revela quais carteirinhas existem.
7. **Tentativas.** Depois de **5 recusas seguidas** para a mesma carteirinha em 15 minutos, essa carteirinha fica bloqueada por 15 minutos, mesmo com a senha certa, com o aviso "Muitas tentativas; tente de novo mais tarde". O controle fica na memória do servidor e some quando ele reinicia. Uma entrada certa zera a contagem. As recusas da API REST contam junto ([API](api.md), item 13).
8. **Inscrição vencida ou suspensão** não impedem a entrada. O leitor precisa ver a própria situação. Os bloqueios valem nas ações (partes c e d), com as mesmas recusas do balcão.

### Sessão (parte a)

9. **Sessão própria.** A sessão do leitor é separada da sessão da equipe:
   - o cookie é `elzevir_patron`, com `HttpOnly` e `SameSite=Lax`, e com `Secure` fora do modo de teste;
   - o token anti-falsificação é próprio;
   - a sessão expira depois de 30 minutos sem uso.

   Uma pessoa pode estar ao mesmo tempo na equipe e como leitora no mesmo navegador, sem que uma sessão dê acesso à outra.
10. **Sair.** `POST /conta/sair`, com o token, encerra a sessão e volta ao início.
11. **Muro.** A página do registro, guardada em cache por versão, nunca leva dado de leitor. O início do OPAC mostra no painel só o primeiro nome de quem entrou e é servido com `Cache-Control: no-store`. A página da conta também.

### Página da conta

12. `GET /conta` mostra o nome, a carteirinha, a unidade, a categoria e a validade da inscrição do próprio leitor. Sem sessão de leitor, leva a `/conta/entrar`.
13. **Empréstimos (parte b).** A página lista os empréstimos do leitor, o primeiro a vencer primeiro, com o título, a data de devolução (com hora só quando não é o fim do dia), a marca "em atraso" quantas vezes foi renovado e o código de barras do exemplar, como no Koha (`opac-user.pl`).
14. **Restrições.** Uma suspensão que vale hoje aparece com o dia em que acaba, ou com o aviso de falar com a biblioteca quando só a equipe a tira. A restrição por atraso aparece como bloqueio por itens em atraso. O leitor não tira restrição nenhuma.
15. **Débitos.** A página mostra o total devido, quando há, e as linhas da conta, a mais nova primeiro: dia, o que é (multa por atraso, item perdido, taxa, pagamento, abono, com o código de barras do item quando a linha é de um item), valor e saldo. O pagamento é só no balcão.
16. Os dados vêm das mesmas consultas da ficha do leitor na equipe, mas abrir a própria conta não grava no `audit_log`: o registro de abertura é para a equipe que vê dado alheio.

### Renovação (parte c)

17. Cada empréstimo mostra quantas renovações ainda restam pelas regras (Koha: `GetRenewCount`, `renewsleft`) e, quando resta alguma e não é empréstimo local, o botão "Renovar".
18. `POST /conta/renovar`, com o token da sessão do leitor e o código de barras, renova pelo comando `circ.renew`, com as mesmas regras e na mesma ordem do balcão ([renovacao.md](../circulacao/renovacao.md)), sem passar por cima de nada. O comando leva o leitor (`by_patron`) e não tem ator da equipe no `audit_log`.
19. Recusas próprias do OPAC, antes das regras:
   - empréstimo de outra pessoa é tratado como sem empréstimo, para a página não revelar nada sobre ele;
   - leitor com restrição valendo hoje (suspensão ou bloqueio por atraso) não renova pelo OPAC. A página não mostra os botões e diz para renovar no balcão. Koha: `opac-user.pl` esconde a renovação do leitor com `debarred`.
20. Depois de renovar, a página volta para `/conta`. Se deu certo, diz o título e a nova data de devolução; se não, diz o motivo em texto para o leitor.

### Reservas e histórico de leitura (parte d)

21. **Link.** A página do registro no OPAC mostra "Reservar" quando o registro tem exemplares e o OPAC tem contas de leitor. O link é o mesmo para todos (`/conta/reservar/{registro}`) e não depende de quem vê, então a página continua em cache por versão sem dado de leitor (item 11).
22. **Página da reserva.** `GET /conta/reservar/{registro}` pede sessão de leitor (sem ela, leva a `/conta/entrar`) e é servida com `Cache-Control: no-store`. Mostra o título e a unidade de retirada, com a unidade do leitor já escolhida.
23. **Pedir.** `POST /conta/reservar/{registro}`, com o token da sessão do leitor, faz uma **reserva de registro** pelo comando `circ.place_hold`, com as mesmas regras e na mesma ordem do balcão ([reservas.md](../circulacao/reservas.md), itens 3 e 4), sem passar por cima de nada. O comando não tem ator da equipe no `audit_log`. Se deu certo, volta para `/conta` com o aviso; se não, a página da reserva volta com o motivo em texto para o leitor (status 422).
24. **Lista.** `/conta` lista as reservas do leitor, na ordem da ficha da equipe, com o título, a unidade de retirada e a situação: posição na fila, em trânsito ou "à sua espera até" o último dia de retirada.
25. **Cancelar.** `POST /conta/reservas/{reserva}/cancelar`, com o token, cancela pelo comando `circ.cancel_hold` marcado como do leitor. O leitor só cancela reserva própria e ainda não separada: a reserva já separada (à espera ou em trânsito) é recusada e só o balcão a cancela. Koha: `Koha::Hold::is_cancelable_from_opac`.
26. **Histórico de leitura.** `/conta` diz se a biblioteca guarda os exemplares que o leitor devolve e oferece o botão para mudar. `POST /conta/historico`, com o token, grava a escolha pelo comando `patrons.set_reading_history`, sobe o `row_version` do leitor e não tem ator da equipe no `audit_log`.
27. A escolha vale para as próximas devoluções. Parar de guardar não apaga o histórico já guardado, como no Koha (`opac-privacy.pl`, preferência `OPACPrivacy`); apagar o histórico antigo fica para depois.

## 2. Origem no Koha

- Entrada: `C4::Auth::checkpw` e `opac-user.pl`. No Koha, o leitor entra pelo `userid` ou pela carteirinha, e aqui entra só pela carteirinha.
- Tentativas: preferência `FailedLoginAttempts`. O Koha trava a conta no banco até a equipe destravar. Aqui o bloqueio é temporário e fica na memória do servidor, porque um bloqueio permanente deixaria qualquer pessoa trancar a conta de outra só errando a senha.
- Tamanho da senha: `minPasswordLength` e `RequireStrongPassword`. Aqui o mínimo é fixo em 8 caracteres.
- Renovação: `opac-renew.pl` e `opac-user.pl`. `OpacRenewalAllowed` fica fixo ligado. `BlockExpiredPatronOpacActions` vazio, como no padrão do Koha: inscrição vencida não impede renovar. Como `ReturnBeforeExpiry` está desligado, a nova data pode passar da validade da inscrição.
- Reservas: `opac-reserve.pl` e `Koha::Hold::is_cancelable_from_opac`. `OPACHoldRequests` e `OPACAllowUserToChooseBranch` ficam fixos ligados; `OPACAllowHoldDateInFuture`, fixo desligado. O leitor não escolhe exemplar.
- Histórico de leitura: `opac-privacy.pl` e `OPACPrivacy`, ligado.

Não é área de paridade.

## 3. Fora desta versão

- O leitor trocar a própria senha.
- Recuperar a senha por e-mail.
- Entrar pelo CPF.
- Configurar as regras de senha no `elzevir.toml`.
- Reservar um exemplar escolhido, ou com data futura, pelo OPAC.
- Apagar o histórico de leitura já guardado.
