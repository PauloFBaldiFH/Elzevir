# Leituras com hooks de diagnóstico — base da P3

## Comportamento

`ReadPool::read_with_hooks` empresta uma conexão existente do pool com acesso
mutável aos hooks locais da conexão. A abertura SQLite permanece somente de
leitura: o acesso mutável não permite criar tabelas nem gravar dados.

O chamador pode instalar um progress handler para interromper uma consulta de
diagnóstico. Deve retirar o handler antes de devolver a conexão; a próxima
leitura usa a mesma conexão normalmente. A API `read` continua disponível e
delega ao mesmo mecanismo de empréstimo, sem abrir conexões adicionais.

## Origem e paridade

Não porta comportamento do Koha. É a base de leitura necessária ao diagnóstico
P3, mantendo a escritora única do doc 04. A integração com IPC e painel fica
nos recortes seguintes. Habilita apenas a feature hooks do rusqlite existente.

## Verificação

Um pool de uma conexão cancela uma consulta pelo progress handler, retira o
handler, recusa CREATE TABLE com SQLITE_READONLY e permite a leitura seguinte.
