# Migração do Koha, parte 1: unidades, categorias e leitores (v0)

> Status: v0, tarefa 5.1 do [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Fase 5, aprovada por Paulo em 2026-11-26). Parte a: leitura do dump, unidades, categorias, leitores e restrições. Parte b: senhas do OPAC (item 12a). Base: [leitores](../circulacao/leitores.md), [suspensão](../circulacao/suspensao.md), [tabelas de códigos](../catalogo/tabelas-de-codigos.md) (a tabela de borda `koha_codes`) e a [importação do Koha em SQL](../catalogo/importacao-universal.md). Critério: **o dump do oráculo migra sem perda nos campos que o Elzevir guarda, com um relatório do que ficou de fora**.

A biblioteca que troca o Koha pelo Elzevir traz o acervo pela importação (MARC ou SQL) e, depois, os leitores, com este comando. Os códigos do Koha (unidade, categoria, número do leitor) ficam só nas tabelas de borda; o núcleo continua sem siglas.

## 1. Comportamento

### O comando

1. `elzevir migrate-koha --data-dir DIR DUMP` lê um dump completo do banco do Koha 26.05, como o `koha-dump` grava (`mysqldump --databases`, comprimido em gzip ou não; o gzip é reconhecido pelos dois primeiros bytes). Trava a pasta de dados como os outros papéis (linha de comando, item 10): com o servidor no ar, nada é lido.
2. Só o fuso de `elzevir.toml` (`[library] time_zone`) é lido da configuração; "hoje" é o dia de agora nesse fuso.
3. O dump é lido em fluxo, com a mesma gramática fechada da importação em SQL (`CREATE TABLE`, `INSERT`, `DROP`, `LOCK`, `UNLOCK`, `SET NAMES`, comentários), mais `CREATE DATABASE` e `USE`, que o `--databases` escreve, e os `SET` de variáveis de sessão e `COMMIT` com que o MariaDB 11 envolve as linhas de cada tabela. Valores hexadecimais (`0x…`, `X'…'`), introdutores de conjunto de caracteres (`_binary '…'`) e números com expoente são aceitos e descartados. Só as linhas de `branches`, `categories`, `borrowers` e `borrower_debarments` são guardadas, e só nas colunas usadas; o resto do banco (acervo, empréstimos, logs) passa sem ficar na memória. Limites: 8 GiB de dump, 16 MiB por comando, 1 MiB por valor guardado, 1.000.000 de leitores.
4. Dump que não tem `borrowers` é recusado sem gravar nada ("no Koha patrons in this dump"). Erro de gramática ou de limite para antes de gravar, com o byte onde parou e sem nenhum valor do dump na mensagem.

### Unidades

5. Cada `branches` vira uma unidade: `branchname` como nome; `branchaddress1` a `3`, `branchcity`, `branchstate`, `branchzip` e `branchcountry` juntos, separados por vírgula, como endereço; `branchphone`, `branchemail` e `branchurl`. A unidade que já tem esse código na tabela de borda (criada pela importação do acervo ou pelo `koha-codes`) recebe esses dados; a que não tem é criada com o código.

### Categorias

6. Cada `categories` vira uma categoria de leitor, com `description` como nome (o código, quando vazio) e o período de `enrolmentperiod` (meses) ou `enrolmentperioddate` (data). Sem nenhum dos dois, ou com meses fora de 1 a 999, fica com 12 meses, e o relatório conta.
7. A categoria já migrada (o código está na tabela de borda) fica como está. Senão, uma categoria do Elzevir com o mesmo nome é usada; senão, é criada. O código vai para a tabela de borda `koha_keys`.

### Leitores

8. Cada `borrowers` vira um leitor:

   | Elzevir | Koha |
   |---|---|
   | Carteirinha | `cardnumber` |
   | Sobrenome | `surname`; vazio, `firstname`, que então não vai para o nome |
   | Nome | `firstname` |
   | Nascimento | `dateofbirth` |
   | E-mail | `email`; vazio, `emailpro` |
   | Telefone | `phone`; vazio, `mobile`; vazio, `phonepro` |
   | Endereço | `streettype` e `address`, `streetnumber`, `address2`, `city`, `state`, `zipcode`, `country`, os preenchidos separados por vírgula |
   | Unidade, categoria | `branchcode`, `categorycode`, pela tabela de borda |
   | Inscrição | `dateenrolled`; vazio, hoje |
   | Validade | `dateexpiry`; vazio, calculada pela categoria (leitores, item 5) |
   | Observação interna | `borrowernotes` |
   | Histórico de leitura | guardado só com `privacy = 0` ("para sempre", a escolha explícita do leitor no Koha); `1` (o padrão da biblioteca) e `2` (nunca) viram **não guardar** (leitores, §5) |

9. Data `0000-00-00` ou que não é data conta como vazia.
10. Um campo opcional que quebra a regra do Elzevir (leitores, item 4: tamanho, e-mail sem `@`, nascimento depois da inscrição, validade antes da inscrição) fica vazio, e o relatório conta por campo. A validade vazia vem da categoria.
11. Leitor sem carteirinha, sem nome nenhum, com carteirinha de mais de 32 caracteres, com unidade ou categoria que o dump não tem, ou cuja carteirinha já é de outro leitor do Elzevir, **não entra**; o relatório traz o `borrowernumber` e o motivo.
12. O CPF não vem: o Koha não tem a coluna.
12a. **Senha do OPAC** (parte b). O hash bcrypt do Koha (`borrowers.password` que começa com `$2`) vem como está, porque a senha não existe fora dele. Senha vazia ou `!` (conta travada) é leitor sem senha. Hash num formato mais antigo (MD5 do Koha de antes de 3.12) não vem, e o relatório conta. Rodar de novo dá a senha a quem foi migrado sem ela e não mexe em quem já tem uma. No primeiro login com ela, o OPAC troca pelo hash Argon2id ([conta do leitor](../opac/conta-do-leitor.md), item 4a).
13. O `borrowernumber` de cada leitor migrado vai para `koha_keys`, que a migração dos empréstimos (5.2) vai usar. Rodar de novo pula quem já foi migrado (o relatório conta), então uma migração interrompida pode ser repetida.

### Restrições

14. Cada `borrower_debarments` de um leitor migrado, com `expiration` vazia ou a partir de hoje, vira uma restrição. `OVERDUES` vira a restrição por atraso; `SUSPENSION`, `MANUAL` e `DISCHARGE` viram suspensão (o Elzevir não tem os outros dois tipos; os três impedem o empréstimo). `9999-12-31` é "até a equipe levantar".
15. Como o Elzevir guarda uma restrição de cada tipo por leitor, as de um mesmo tipo se juntam: sem fim ganha; senão, o fim mais tarde (Koha: `UpdateBorrowerDebarmentFlags`). O momento de criação é o `created` mais antigo, no fuso da biblioteca.

### Gravação e relatório

16. A gravação vai pela escritora, em comandos `patrons.migrate_koha` de até 500 leitores, cada um numa transação; o primeiro traz também as unidades e as categorias. Cada comando vai para o `audit_log` inteiro, como todo comando de leitor (leitores, §6).
17. O relatório vai para a saída padrão, em inglês, **sem dado pessoal**: contagens de unidades e categorias criadas e atualizadas, leitores migrados, já migrados e recusados (com `borrowernumber` e motivo), restrições, senhas migradas e deixadas de fora, campos esvaziados por campo e, para cada coluna de `borrowers` que o Elzevir não guarda (endereço alternativo, contatos, `sex`, `opacnote`, `sort1`, `sort2`, `userid` e as outras), quantos leitores a tinham preenchida.
18. Leitores com permissão de equipe no Koha (`flags`) entram como leitores; contas da equipe se criam com `elzevir staff add`. O relatório conta quantos tinham permissão.

## 2. Casos de borda

19. Dump com dois `CREATE TABLE borrowers` é recusado (declaração repetida), como na importação em SQL. `INSERT` sem colunas antes do `CREATE TABLE` da tabela é recusado para as quatro tabelas.
20. Leitor com `categorycode` que não está em `categories` é recusado (item 11), mesmo que exista uma categoria do Elzevir com esse nome.

## 3. Origem no Koha

- `installer/data/mysql/kohastructure.sql` (tabelas `branches`, `categories`, `borrowers`, `borrower_debarments`) e `debian/scripts/koha-dump`.
- `Koha::Patron::Debarments::UpdateBorrowerDebarmentFlags` (item 15) e `opac-privacy.pl` (`privacy`: 0 para sempre, 1 padrão, 2 nunca).

## 4. Paridade

`livre` (migração). Prova: testes de unidade da leitura do dump e da tradução (`elzevir-app`), testes da gravação (`crates/elzevir-store/tests/koha_migration.rs`) e um teste do binário com um dump sintético, com leitores fictícios, no formato do `koha-dump` (`crates/elzevir/tests/cli.rs`). A conferência com o dump do oráculo usa um script que Paulo roda na VM, sem tirar dado de leitor de lá.

## 5. Desvios

- `MANUAL` e `DISCHARGE` viram suspensão (item 14). A quitação do Koha (`DISCHARGE`) bloqueia o leitor depois de devolver tudo; aqui ele fica suspenso até a equipe levantar, com o mesmo efeito no balcão.
- `privacy = 1` (o padrão do Koha, que guarda o histórico) vira "não guardar", porque o Elzevir só guarda com consentimento.
