# Conta do leitor no OPAC

> Status: tarefa 3.10, parte a (senha e entrada). Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 3, linha 3.10) e [interface](../../UI_BIBFRAME_INTERACTION_SPEC.md) §23 item 6. As partes seguintes trazem empréstimos e débitos (b), renovação (c) e reservas e histórico de leitura (d).

## 1. Comportamento

### Senha do leitor (parte a)

1. **Sem senha, sem conta.** Um leitor só entra no OPAC depois que a equipe define uma senha para ele. O leitor nasce sem senha.
2. **Quem define.** A senha é definida na edição do leitor, por quem tem a permissão de leitores, pelo comando `patrons.set_password`. Uma senha nova substitui a anterior. Também dá para tirar a senha, e aí o leitor não entra mais.
3. **Regra da senha.** A senha tem de 8 a 128 caracteres. Não pode ser igual à carteirinha nem ao CPF.
4. **Guarda.** O servidor guarda só o hash Argon2id, nunca a senha. O comando que vai para o `audit_log` leva só o hash.

### Entrada (parte a)

5. **Como entra.** O leitor entra pelo número da carteirinha e pela senha, no painel "Conta do leitor" do início do OPAC ou em `GET /conta/entrar`. O formulário vai para `POST /conta/entrar` e nunca pede o nome.
6. **Recusa igual.** Carteirinha desconhecida, leitor sem senha e senha errada dão a mesma recusa, "Carteirinha ou senha incorretas". O tempo de resposta também é o mesmo, porque o servidor confere um hash de mentira quando não há hash de verdade. Assim, o OPAC não revela quais carteirinhas existem.
7. **Tentativas.** Depois de **5 recusas seguidas** para a mesma carteirinha em 15 minutos, essa carteirinha fica bloqueada por 15 minutos, mesmo com a senha certa, com o aviso "Muitas tentativas; tente de novo mais tarde". O controle fica na memória do servidor e some quando ele reinicia. Uma entrada certa zera a contagem.
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
16. Os dados vêm das mesmas consultas da ficha do leitor na equipe, mas abrir a própria conta não grava no `audit_log`: o registro de abertura é para a equipe que vê dado alheio. Reservas entram na parte d.

## 2. Origem no Koha

- Entrada: `C4::Auth::checkpw` e `opac-user.pl`. No Koha, o leitor entra pelo `userid` ou pela carteirinha, e aqui entra só pela carteirinha.
- Tentativas: preferência `FailedLoginAttempts`. O Koha trava a conta no banco até a equipe destravar. Aqui o bloqueio é temporário e fica na memória do servidor, porque um bloqueio permanente deixaria qualquer pessoa trancar a conta de outra só errando a senha.
- Tamanho da senha: `minPasswordLength` e `RequireStrongPassword`. Aqui o mínimo é fixo em 8 caracteres.

Não é área de paridade.

## 3. Fora desta versão

- O leitor trocar a própria senha.
- Recuperar a senha por e-mail.
- Entrar pelo CPF.
- Configurar as regras de senha no `elzevir.toml`.
