# LGPD: ferramentas de privacidade do leitor

> Status: tarefa 5.4 completa: partes a (anonimizar um leitor), b (exportar os dados de um leitor) e c (prazos de guarda do histórico e dos leitores vencidos). Base: Koha 26.05.05, [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (tabela da Fase 5, linha 5.4) e [leitores.md](leitores.md) §6.

## 1. Comportamento

### Anonimizar um leitor

1. O comando `patrons.anonymize` recebe o id do leitor. É definitivo: não há como desfazer, e o próprio comando no `audit_log` guarda só o id.
2. É recusado enquanto o leitor tem empréstimo em aberto (`patron-error-has-loans`) ou reserva ativa (`patron-error-has-holds`), e para leitor que não existe ou já foi anonimizado. Quando passa, o cadastro perde carteirinha, sobrenome, nome, CPF, nascimento, e-mail, telefone, endereço, observação da equipe e senha do OPAC. A carteirinha passa a ser `anon-` seguido do id (é única e obrigatória), o sobrenome fica vazio, o histórico de leitura fica desligado e `anonymized_at` guarda o momento. Unidade, categoria, inscrição e validade ficam, para as estatísticas.
3. Os empréstimos devolvidos (`returned_loans`) e as reservas encerradas (`ended_holds`) ficam, sem o leitor. A conta (débitos, créditos e o que foi pago) fica ligada ao registro anonimizado, porque é registro financeiro da biblioteca.
4. Os avisos já gerados ficam (o que foi avisado e quando), sem o endereço para onde foram.
5. **`audit_log`.** Todo comando registrado que traz a carteirinha ou o CPF do leitor perde, no objeto que é o leitor, os campos pessoais (os do item 2, e a senha em hash da migração do Koha). O resto do comando fica, e um homônimo, de outra carteirinha e sem o mesmo CPF, não é tocado. Os identificadores vêm do cadastro atual e das edições registradas (`patrons.update`); um objeto que é o leitor e traz outro identificador dele (a carteirinha antiga ao lado do CPF) faz esse identificador ser procurado também, até não aparecer nenhum novo.
6. **Na tela.** O formulário de edição (`/equipe/leitores/{id}/editar`, permissão de leitores) tem a seção "Anonimizar", com o que será apagado e uma caixa "Entendo que não dá para desfazer". O envio vai para `POST /equipe/leitores/{id}/anonimizar`; sem a caixa marcada, volta 422 com o aviso; uma recusa volta ao formulário com a mensagem (422). Feito, a página redireciona (303) para a busca de leitores com o aviso "O leitor foi anonimizado".
7. Um leitor anonimizado some da busca, da ficha (404), do formulário, do login do OPAC e do SIP2, e uma nova rodada da migração do Koha não lhe devolve empréstimos nem reservas.

### Exportar os dados de um leitor

8. O comando `patrons.export` recebe o id do leitor e só registra, no `audit_log`, que a equipe tirou a cópia (quem e quando), como a abertura da ficha. Leitor que não existe ou foi anonimizado é recusado.
9. A cópia é um documento JSON (`"format": "elzevir-patron-export/1"`) com tudo o que o Elzevir guarda do leitor:
   - `patron`: o cadastro inteiro, com o CPF completo e os nomes da unidade e da categoria; a senha do OPAC aparece só como `opac_password` (sim ou não), nunca o hash;
   - `loans`, `returned_loans`, `holds`, `ended_holds`: empréstimos em aberto e devolvidos, reservas ativas e encerradas, cada exemplar com código de barras e título;
   - `account_lines` e `account_offsets`: a conta, linha a linha, e os movimentos entre as linhas;
   - `restrictions`, `notices` e `notice_items`: suspensões e bloqueios, avisos gerados e os exemplares de cada aviso;
   - `record_access`: quando a ficha foi aberta, o CPF mostrado ou a cópia tirada, sem dizer por quem (o login é dado do funcionário, não do leitor).
   Cada lista traz todas as colunas da tabela, de modo que uma coluna nova entra sozinha.
10. O registro de auditoria entra só como `record_access`; os comandos de cadastro e edição já estão representados pelo próprio cadastro.
11. **Na tela.** O formulário de edição (permissão de leitores) tem a seção "Dados do leitor", com o botão "Baixar dados", que envia para `POST /equipe/leitores/{id}/exportar`. A resposta é o arquivo `leitor-{id}.json` para download (`Content-Disposition: attachment`, `Cache-Control: no-store`). O registro no `audit_log` acontece antes de os dados saírem do servidor. Cabe à biblioteca entregar o arquivo ao leitor que pedir (LGPD, art. 18, II, e art. 19).

### Prazos de guarda

12. Dois prazos em `[privacy]` no `elzevir.toml`, em dias, os dois desligados por padrão:

    ```toml
    [privacy]
    reading_history_days = 730      # histórico com leitor por até 2 anos
    anonymize_expired_days = 1095   # leitor vencido há 3 anos é anonimizado
    ```

    Chave desconhecida na seção é erro ao subir o servidor.
13. **Histórico.** Com `reading_history_days`, o empréstimo devolvido e a reserva encerrada cujo dia (no fuso da biblioteca) é anterior a hoje menos o prazo perdem o leitor, mesmo de quem consentiu em guardar o histórico. O registro fica, sem o leitor, como no item 3.
14. **Leitores vencidos.** Com `anonymize_expired_days`, o leitor cuja validade é hoje menos o prazo, ou antes, é anonimizado como no item 2, com a limpeza do `audit_log` (item 5). Fica como está, e é contado à parte, o leitor com empréstimo em aberto, reserva ativa ou saldo devedor na conta: a equipe resolve a pendência e a rodada seguinte o anonimiza.
15. **Quando roda.** O servidor aplica os prazos ao subir e depois de hora em hora, junto com as multas e os avisos, pelo comando `patrons.apply_retention`; o comando só trabalha na primeira rodada de cada dia (`daily_runs`). O log operacional registra `patrons.retention_applied` com quantos registros do histórico perderam o leitor, quantos leitores foram anonimizados e quantos ficaram por pendência, ou `patrons.retention_failed`.
16. O leitor anonimizado nunca é apagado: o registro fica para as estatísticas e para a conta.

## 2. Origem no Koha

- `Koha::Patron::anonymize` (`reference/koha/Koha/Patron.pm`): recusa com empréstimos, limpa as colunas pessoais e marca `anonymized`. No Koha, campos obrigatórios recebem texto aleatório; aqui a carteirinha vira `anon-{id}` e os outros ficam vazios.
- O Koha não tem uma cópia dos dados de um leitor; a preferência `GDPR_Policy` só pede, no OPAC, o consentimento à política de privacidade.
- `Koha::Old::Checkouts::anonymize` e `misc/cronjobs/batch_anonymise.pl --days`: o histórico fica, sem o leitor (item 13).
- `misc/cronjobs/cleanup_database.pl` com `PatronAnonymizeDelay` (`Koha::Patrons::search_anonymize_candidates`): anonimiza quem venceu há mais dias que o prazo (item 14). `LockExpiredDelay` e `PatronRemovalDelay` não têm equivalente (item 16 e desvios).

## 3. Paridade

`livre`. Prova: `crates/elzevir-store/tests/patrons.rs` (carteirinha trocada e homônimo poupado no `audit_log`, segunda anonimização recusada), `crates/elzevir-store/tests/koha_migration.rs` (recusa com empréstimos, histórico e conta mantidos sem o leitor, `audit_log` da migração limpo só para o leitor, nova rodada da migração, exportação com cadastro sem hash, empréstimos, reservas, conta e `record_access`, nada de outro leitor, e cópia recusada depois de anonimizar), teste de unidade de `patron_privacy` e `crates/elzevir/tests/patrons.rs` (permissão, download com cabeçalho de anexo, confirmação, redirecionamento, ficha 404 e busca vazia). Prazos: `crates/elzevir-store/tests/patrons.rs` (histórico antigo sem o leitor e o recente com ele, só o vencido sem pendência anonimizado, dívida e empréstimo poupam, a mesma data não repete e o dia seguinte roda), teste de unidade de `patron_retention` (contagem dos dias) e `crates/elzevir/src/config.rs` (a seção `[privacy]`).

## 4. Casos de borda

1. Leitor que trocou de carteirinha e manteve o CPF: a inscrição com a carteirinha antiga é achada pelo CPF.
2. Leitor que trocou de carteirinha e tirou o CPF na mesma edição: a edição registrada traz a carteirinha nova; a antiga só é achada se outro registro a ligar a ele (item 5).
3. Leitor com dívida em aberto: a anonimização pela equipe é aceita, e a dívida continua na conta do registro anonimizado; o prazo do item 14 não o anonimiza enquanto deve.
4. Prazo de histórico de 1 dia: o devolvido ontem fica; o de anteontem perde o leitor.

## 5. Desvios

- O Koha anonimiza leitor e histórico em passos separados (`anonymize` e `batch_anonymise.pl`); aqui um só comando faz os dois para o leitor.
- O Koha não mexe no `action_logs`; o Elzevir limpa o `audit_log` (item 5).
- No Koha, `batch_anonymise.pl` poupa quem escolheu guardar o histórico "para sempre" (`privacy = 0`); no Elzevir a escolha do leitor é só guardar ou não, e o prazo da biblioteca vale para todos (item 13).
- O Koha só anonimiza o leitor vencido que também está bloqueado (`LockExpiredDelay`) e não olha a dívida; o Elzevir não bloqueia contas e poupa quem deve (item 14).
- O Koha pode apagar o leitor anonimizado (`PatronRemovalDelay`); o Elzevir o mantém (item 16).

## 6. Limites

Ficam fora do alcance do comando: os logs do servidor em disco, os backups feitos antes e as cópias impressas de avisos e recibos. A biblioteca cuida deles pelo seu prazo de guarda.
