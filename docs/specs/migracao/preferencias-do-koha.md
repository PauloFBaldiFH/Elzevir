# Preferências do Koha para o `elzevir.toml` (v0)

> Status: v0, tarefa 5.3 do [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Fase 5, aprovada por Paulo em 2026-11-26), com o desenho do §3.4 do mesmo documento. Base: a [triagem das preferências](../../arquitetura/06-triagem-sysprefs.md) (`tools/sysprefs-triage.json`) e a leitura do dump da [migração dos leitores](leitores-do-koha.md). Critério: **as preferências do oráculo viram um `elzevir.toml` válido e o resto aparece listado para revisão**.

O Koha guarda cerca de 900 preferências do sistema, em texto e sem tipo. O Elzevir não as copia: só vira campo do `elzevir.toml` o que tem justificativa (doc 06). Este comando traz as que já têm campo e separa, para a biblioteca revisar, as que ela mudou em relação ao padrão do Koha.

## 1. Comportamento

### O comando

1. `elzevir import-koha-prefs DUMP` lê a tabela `systempreferences` (colunas `variable` e `value`) de um dump do banco do Koha 26.05, como na migração dos leitores: o dump inteiro do `koha-dump` ou só a tabela (`mysqldump`), comprimido em gzip ou não. As outras tabelas são puladas sem ser guardadas. Não abre pasta de dados nem grava nada: escreve o `elzevir.toml` na saída padrão e o relatório na saída de erro, e a biblioteca redireciona (`elzevir import-koha-prefs koha.sql.gz > elzevir.toml`) ou copia o que quiser para o arquivo que já tem.
2. Dump sem a tabela, ou com a mesma preferência duas vezes, é recusado, com a posição. Valor `NULL` conta como vazio.
3. Antes de escrever, o texto passa pela mesma leitura que o servidor faz ao subir (chave desconhecida, valor fora da regra). O que não passaria não é escrito.

### Para onde vai cada preferência

4. **Com campo.** As preferências que a triagem liga a um campo (`campo` em `tools/sysprefs-triage.json`) vão para ele:
   - `DefaultLongOverdueDays` e `DefaultLongOverdueLostValue`, as duas preenchidas, viram `[circulation.long_overdue]` com `days` e `lost_status`, como o `longoverdue.pl` as lê quando roda sem `--lost`; `charge = true` quando `DefaultLongOverdueChargeValue` é igual à situação de perda, porque o Koha só cobra nesse caso ([conta](../circulacao/conta.md), itens 20 a 22). Com só uma das duas, o Koha não roda a rotina, e a seção não é escrita.
   - `ExpireReservesMaxPickUpDelay` ligada (valor que não é vazio nem `0`, como o Perl a lê) vira `expire_waiting = true` em `[circulation.holds]` ([reservas](../circulacao/reservas.md), item 18).
   - `SuspendHoldsOpac` desligada (`0`) vira `patrons_suspend = false` em `[circulation.holds]` ([conta do leitor](../opac/conta-do-leitor.md), item 25a). Ligada é o padrão dos dois lados e não é escrita.
   - `PatronAnonymizeDelay` preenchida vira `anonymize_expired_days` em `[privacy]` ([LGPD](../circulacao/lgpd.md), item 14).
   - `OAI-PMH` ligada escreve `[oai]` com `repository_identifier` (`OAI-PMH:archiveID`), `max_count` (`OAI-PMH:MaxCount`), `repository_name` (`LibraryName`), `admin_email` (`KohaAdminEmailAddress`) e `base_url` (`OPACBaseURL` com `/oai` no fim), os que estiverem preenchidos, mas com `enabled = false` e um comentário: o endereço do repositório muda (o Koha responde em `/cgi-bin/koha/oai.pl`), e os coletores precisam ser avisados antes de ligar ([OAI-PMH](../opac/oai-pmh.md), item 2). Com `OAI-PMH` desligada, essas preferências seguem a regra dos itens 5 a 7.
5. **No padrão.** A preferência sem campo cujo valor é igual ao padrão do Koha 26.05 (`installer/data/mysql/mandatory/sysprefs.sql`, com os escapes do SQL decodificados) é descartada, como a que tem campo e fica no padrão.
6. **Fora do padrão.** A preferência sem campo e com valor diferente do padrão, a que tem campo mas cujo valor não vira o campo (número inválido, situação de perda zero, metade do par do atraso longo) e a que o Koha 26.05 não conhece vão para `[koha_legacy]`, nome = valor em texto, cada uma com um comentário que traz o padrão do Koha ou diz que ela não é do Koha 26.05. O servidor aceita a seção e não a lê: ela é a lista de revisão, e a [página das preferências](sysprefs-do-koha.md) diz onde cada uma fica no Elzevir.
7. **Senhas e chaves.** As preferências marcadas como `segredo` na triagem (como `AutoSelfCheckPass`, `SMSSendPassword` e `GoogleOAuth2ClientSecret`) nunca são escritas. Fora do padrão, aparecem em `[koha_legacy]` só como comentário, sem o valor.
8. **Relatório.** Sem valor nenhum, só nomes e contagens: preferências lidas, descartadas no padrão, levadas a cada campo, em `[koha_legacy]` (as que não são do Koha 26.05 à parte), senhas não copiadas e, quando houver, o lembrete de revisar e ligar o `[oai]`.

### A página

9. [`sysprefs-do-koha.md`](sysprefs-do-koha.md), "Para administradores do Koha: cada syspref ao lado do seu campo", lista as preferências do Koha 26.05 por aba da tela de preferências, com o padrão e o lugar no Elzevir (o campo, ou o destino e a nota da triagem). É gerada por `cargo xtask codegen` a partir do inventário e da triagem, e o CI (`codegen --check`) falha se ficar desatualizada.

## 2. Casos de borda

1. Escapes do `sysprefs.sql`: `\n` é quebra de linha e `''` é aspa, como o MySQL grava; o padrão é comparado já decodificado.
2. Nome com caixa diferente (`opacuserjs` em vez de `OPACUserJS`): é a mesma preferência, porque no Koha a coluna não diferencia caixa; duas linhas que só diferem na caixa são a mesma preferência repetida (item 2).
3. Valor com aspas, barra invertida ou várias linhas: escrito como texto TOML com os escapes, e lido de volta igual.

## 3. Origem no Koha

- `systempreferences` (`variable`, `value`) e os padrões de `installer/data/mysql/mandatory/sysprefs.sql`, no inventário `data/koha-26.05/sysprefs.json`.
- `misc/cronjobs/longoverdue.pl` (leitura de `DefaultLongOverdueDays`, `DefaultLongOverdueLostValue` e `DefaultLongOverdueChargeValue`), `C4::Reserves::CancelExpiredReserves` (`ExpireReservesMaxPickUpDelay`), `Koha::Patrons::search_anonymize_candidates` (`PatronAnonymizeDelay`), `Koha::OAI::Server::Repository` (`OAI-PMH`, `OAI-PMH:archiveID`, `OAI-PMH:MaxCount`, `LibraryName`, `KohaAdminEmailAddress`).

## 4. Paridade

`livre` (configuração é área de melhoria, CLAUDE.md). Prova: testes de unidade em `elzevir-app::koha_prefs` (cada campo, padrão descartado, fora do padrão, desconhecida, senha, valor inválido, par incompleto, escapes) e em `elzevir-data` (padrões decodificados), em `crates/elzevir/src/config.rs` (`[koha_legacy]` aceita só texto) e `crates/elzevir/tests/cli.rs` (o comando sobre um dump inteiro com preferências no padrão, fora dele, desconhecida e senha gera o `elzevir.toml` e o relatório).

## 5. Desvios

- O doc 03 chama a seção de `[koha_legado]`; ela se chama `[koha_legacy]`, como as outras seções do `elzevir.toml`, que são em inglês.
- O Koha guarda também a configuração do `koha-conf.xml` (fuso, Z39.50, SMTP); ela não está no banco e não é lida.
