# Troca do Koha: o roteiro do dump ao servidor no ar (v0)

> Status: v0, tarefa 6.1 do [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Fase 6, ensaio da troca real, aprovada por Paulo em 2026-11-27). Base: [leitores](leitores-do-koha.md), [circulação](circulacao-do-koha.md) e [preferências](preferencias-do-koha.md) do Koha, e a [importação do Koha em SQL](../catalogo/importacao-universal.md) (§1.8). Critério: **o roteiro roda do dump até o servidor no ar sem passo manual fora dele**.

Até a Fase 5, trocar o Koha pedia quatro comandos e dois arquivos. O acervo vinha de uma exportação MARC (`export_records.pl`) ou de um dump parcial, porque a importação recusa o dump inteiro do banco. Os leitores e a circulação vinham do `koha-dump` pelo `migrate-koha`, que precisava rodar duas vezes: os empréstimos esperam o acervo. Este comando faz tudo isso numa ordem só, a partir do dump que o `koha-dump` grava.

## 1. Comportamento

### O roteiro

1. A troca tem três comandos, nesta ordem:
   1. `elzevir init --data-dir DIR --name NOME` cria a pasta de dados e o primeiro administrador, como em qualquer instalação;
   2. `elzevir switch-from-koha --data-dir DIR DUMP` traz tudo do Koha (itens 2 a 12);
   3. `elzevir serve --data-dir DIR` sobe o servidor.
   O backup depois da troca é o comando de sempre (`elzevir backup create`). A [seção 5](#5-roteiro-do-ensaio) traz o roteiro do ensaio com os cuidados da LGPD.
2. `switch-from-koha` lê o `DUMP` como o `migrate-koha` (dump inteiro do banco, `mysqldump --databases`, com ou sem gzip) e trava a pasta de dados: com o servidor no ar, nada é lido.
3. **Pasta limpa.** O comando recusa uma pasta que já tem Instâncias no acervo, com a mensagem: a troca parte de uma pasta nova, para que nada do Koha se misture com um acervo que já existe. Leitores e circulação já migrados por um `migrate-koha` anterior também fazem a pasta ser recusada.
4. **Ler tudo antes de gravar.** O dump é lido três vezes, uma por leitura (preferências, acervo, migração), em vez de ser guardado inteiro na memória; cada leitura guarda só as tabelas de que precisa, com os limites de cada uma. Só depois das três leituras darem certo algo é gravado: um dump com defeito não deixa a pasta pela metade.

### Os passos, em ordem

5. **Preferências.** As preferências viram `elzevir.toml` como no `import-koha-prefs`. Se a pasta ainda não tem `elzevir.toml`, ele é escrito ali. Se já tem, o arquivo da biblioteca fica como está e o resultado vai para `elzevir.koha.toml`, ao lado, para revisão. Em nenhum caso um arquivo existente é sobrescrito; um `elzevir.koha.toml` antigo é recusado.
6. **Unidades, categorias e leitores.** Primeira parte do `migrate-koha` (itens 4 a 16 de [leitores-do-koha.md](leitores-do-koha.md)): as unidades entram antes do acervo, para que os exemplares as encontrem pelo código do Koha.
7. **Tipos de item.** Os tipos de item que os exemplares usam e a borda MARC ainda não conhece são criados como no `import --create-codes`, mas com o nome que o Koha dá (`itemtypes.description`), e não com o código. Um tipo sem linha em `itemtypes` fica com o código como nome. Os tipos que só as regras de circulação usam (item 7a) também são criados.
7a. **Regras de circulação** (tarefa 6.3a). Cada linha de `circulation_rules` vira uma regra do Elzevir com o mesmo nome (`rule_name`) e o mesmo valor (`rule_value`, que guarda o vazio e os espaços), no escopo achado pelos códigos do Koha: a unidade por `branchcode`, a categoria por `categorycode` e a política por `itemtype`; `NULL` ou vazio é "qualquer". A regra é conferida como o `circ.set_rules` confere ([regras](../circulacao/regras.md), item 5). Fica de fora, com o `id` e o motivo no relatório, a regra sem `rule_name`, de um tipo que o Koha 26.05 não tem, num escopo que o tipo não usa, com valor longo demais ou monetário que não é decimal, vazia num tipo que não pode ficar vazio (o Koha apaga essa regra) ou com um código que o Elzevir não tem. As regras entram numa escrita só (`patrons.migrate_koha_rules`), depois dos tipos de item e antes do acervo.
8. **Acervo.** `biblio_metadata` e `items` do dump inteiro entram pelo mesmo caminho da importação do Koha em SQL (importação universal, §1.8): mesmo leitor de tabelas, mesma montagem do `952`, mesmo MARCXML guardado como origem e mesma escritora. A diferença está em dois pontos. As outras tabelas do banco são puladas sem guardar valor nenhum, em vez de fazerem o dump ser recusado como backup completo. E o limite é o do dump inteiro (8 GiB), não o do arquivo de importação. Origem `koha-dump`.
9. **Circulação.** O restante do `migrate-koha`: empréstimos, reservas, contas e histórico, que agora encontram os exemplares e as Instâncias.
10. **Índice.** O índice de busca é construído inteiro antes de o comando terminar, para que o primeiro `serve` não comece reindexando.
11. **Relatório.** Sem dado pessoal, como os do `migrate-koha`: para cada passo, o tempo que levou e as contagens (preferências, regras de circulação, registros lidos e gravados, exemplares, recusas pelo número do Koha, leitores, empréstimos, reservas, contas, histórico, documentos no índice), seguidos dos relatórios dos passos 6 e 9 como o `migrate-koha` os escreve.
12. **Falha na gravação.** Cada passo grava em lotes, como os comandos que ele reúne; uma falha da escritora (disco cheio, por exemplo) para o roteiro com o nome do passo, e o que já foi gravado fica. Como a pasta deixou de estar limpa, o ensaio recomeça numa pasta nova.

## 2. Casos de borda

1. Dump sem `biblio_metadata`, sem `borrowers` ou sem `systempreferences`: recusado na leitura, antes de gravar qualquer coisa, nem o `elzevir.toml` (item 4).
2. Exemplar de uma unidade que não está em `branches`: o registro é recusado, como na importação, e o relatório o nomeia pelo `biblionumber`; os empréstimos e reservas dele ficam de fora, com o número do Koha.
3. Registro sem exemplar: entra, como na importação.

## 3. Origem no Koha

- `Koha::CirculationRules::set_rule` (a conferência de cada regra).
- `debian/scripts/koha-dump` (o formato do dump) e as tabelas `biblio_metadata`, `items`, `itemtypes` e `circulation_rules` de `installer/data/mysql/kohastructure.sql`.
- O resto vem das specs de migração e da importação citadas no topo.

## 4. Paridade

`livre` (migração). Prova:
- testes de unidade da leitura do acervo no dump inteiro e dos tipos de item (`elzevir-app`);
- um teste do binário com um dump sintético no formato do `koha-dump`, com leitores fictícios, acervo, empréstimo e preferências: confere o relatório, o `elzevir.toml`, a pasta recusada na segunda vez e a busca encontrando o registro (`crates/elzevir/tests/cli.rs`);
- o ensaio da [seção 5](#5-roteiro-do-ensaio) com a cópia da Castro Alves, cujo relatório (sem dado pessoal) Paulo traz para o projeto.

## 5. Roteiro do ensaio

O ensaio roda na máquina de Paulo ou na VM do oráculo, nunca na nuvem, porque o dump tem leitores reais (LGPD):

1. Gerar o dump na produção com `koha-dump`, copiar para a máquina do ensaio e não guardar em mais nenhum lugar.
2. Rodar `elzevir init` numa pasta nova, `elzevir switch-from-koha` com o dump e depois `elzevir verify-koha` com o mesmo dump ([verificação](verificacao-do-koha.md)), e guardar os dois relatórios, que não têm dado pessoal.
3. Subir o `elzevir serve` e conferir o balcão e o OPAC (tarefas 6.3 e 6.4).
4. Apagar a pasta e o dump no fim do ensaio.

## 6. Desvios

- A importação do acervo pelo dump inteiro, que a importação universal recusa como backup completo, só existe neste comando: a tela de importação continua aceitando só o dump parcial, porque lá o dump passa pelo navegador e por um lote guardado.
