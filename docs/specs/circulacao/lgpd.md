# LGPD: ferramentas de privacidade do leitor

> Status: tarefa 5.4, parte a (anonimizar um leitor). Partes b (exportar os dados de um leitor) e c (prazos de guarda do histórico e de leitores inativos) virão em PRs próprios. Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 5, linha 5.4) e [leitores.md](leitores.md) §6.

## 1. Comportamento

### Anonimizar um leitor

1. O comando `patrons.anonymize` recebe o id do leitor. É definitivo: não há como desfazer, e o próprio comando no `audit_log` guarda só o id.
2. É recusado enquanto o leitor tem empréstimo em aberto (`patron-error-has-loans`) ou reserva ativa (`patron-error-has-holds`), e para leitor que não existe ou já foi anonimizado. Quando passa, o cadastro perde carteirinha, sobrenome, nome, CPF, nascimento, e-mail, telefone, endereço, observação da equipe e senha do OPAC. A carteirinha passa a ser `anon-` seguido do id (é única e obrigatória), o sobrenome fica vazio, o histórico de leitura fica desligado e `anonymized_at` guarda o momento. Unidade, categoria, inscrição e validade ficam, para as estatísticas.
3. Os empréstimos devolvidos (`returned_loans`) e as reservas encerradas (`ended_holds`) ficam, sem o leitor. A conta (débitos, créditos e o que foi pago) fica ligada ao registro anonimizado, porque é registro financeiro da biblioteca.
4. Os avisos já gerados ficam (o que foi avisado e quando), sem o endereço para onde foram.
5. **`audit_log`.** Todo comando registrado que traz a carteirinha ou o CPF do leitor perde, no objeto que é o leitor, os campos pessoais (os do item 2, e a senha em hash da migração do Koha). O resto do comando fica, e um homônimo, de outra carteirinha e sem o mesmo CPF, não é tocado. Os identificadores vêm do cadastro atual e das edições registradas (`patrons.update`); um objeto que é o leitor e traz outro identificador dele (a carteirinha antiga ao lado do CPF) faz esse identificador ser procurado também, até não aparecer nenhum novo.
6. **Na tela.** O formulário de edição (`/equipe/leitores/{id}/editar`, permissão de leitores) tem a seção "Anonimizar", com o que será apagado e uma caixa "Entendo que não dá para desfazer". O envio vai para `POST /equipe/leitores/{id}/anonimizar`; sem a caixa marcada, volta 422 com o aviso; uma recusa volta ao formulário com a mensagem (422). Feito, a página redireciona (303) para a busca de leitores com o aviso "O leitor foi anonimizado".
7. Um leitor anonimizado some da busca, da ficha (404), do formulário, do login do OPAC e do SIP2, e uma nova rodada da migração do Koha não lhe devolve empréstimos nem reservas.

## 2. Origem no Koha

- `Koha::Patron::anonymize` (`reference/koha/Koha/Patron.pm`): recusa com empréstimos, limpa as colunas pessoais e marca `anonymized`. No Koha, campos obrigatórios recebem texto aleatório; aqui a carteirinha vira `anon-{id}` e os outros ficam vazios.
- `Koha::Old::Checkouts::anonymize` e `misc/cronjobs/batch_anonymise.pl`: o histórico fica, sem o leitor.

## 3. Paridade

`livre`. Prova: `crates/elzevir-store/tests/patrons.rs` (carteirinha trocada e homônimo poupado no `audit_log`, segunda anonimização recusada), `crates/elzevir-store/tests/koha_migration.rs` (recusa com empréstimos, histórico e conta mantidos sem o leitor, `audit_log` da migração limpo só para o leitor, nova rodada da migração), teste de unidade de `patron_privacy` e `crates/elzevir/tests/patrons.rs` (permissão, confirmação, redirecionamento, ficha 404 e busca vazia).

## 4. Casos de borda

1. Leitor que trocou de carteirinha e manteve o CPF: a inscrição com a carteirinha antiga é achada pelo CPF.
2. Leitor que trocou de carteirinha e tirou o CPF na mesma edição: a edição registrada traz a carteirinha nova; a antiga só é achada se outro registro a ligar a ele (item 5).
3. Leitor com dívida em aberto: a anonimização é aceita, e a dívida continua na conta do registro anonimizado.

## 5. Desvios

- O Koha anonimiza leitor e histórico em passos separados (`anonymize` e `batch_anonymise.pl`); aqui um só comando faz os dois para o leitor.
- O Koha não mexe no `action_logs`; o Elzevir limpa o `audit_log` (item 5).

## 6. Limites

Ficam fora do alcance do comando: os logs do servidor em disco, os backups feitos antes e as cópias impressas de avisos e recibos. A biblioteca cuida deles pelo seu prazo de guarda.
