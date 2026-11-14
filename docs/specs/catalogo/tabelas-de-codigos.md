# Tabelas de códigos: nomes vindos do Koha e editados na tela (v0)

> Status: v0, pedido de Paulo em 2026-11-14 na revisão da grade de exemplares. Base: [modelo bibliográfico](modelo-bibliografico.md) §2 (tabelas `branches`, `item_types`, `terms`), [exemplares](exemplares.md) item 29 (toda tela mostra o nome, não o código) e [linha de comando](linha-de-comando.md). Critério: **depois de trazer os nomes do Koha, nenhuma tela mostra MAIN, BK ou GEN onde o Koha tem nome**.

Duas partes, cada uma num PR:

- **a, nomes do Koha:** o comando `catalog.name_codes` e o papel `elzevir koha-codes` (itens 1 a 13).
- **b, tela:** a página que lista e renomeia os códigos (itens 14 a 19).

## 1. Comportamento

### As tabelas

1. São quatro: **unidades** (`branches.name`), **tipos de item** (`item_types.description`), **localizações** e **coleções** (`terms.label`, vocabulários `Location` e `Collection`). O código é o que os registros e os lotes guardam e não muda; o nome é o que as pessoas leem.

### Comando

2. `catalog.name_codes` (`CatalogCommand::NameCodes`) recebe uma lista de `(tabela, código, nome)` e, numa transação só:
   - código que já existe na tabela recebe o nome, se for diferente;
   - código que não existe é criado com esse nome (assim a lista da grade passa a ter as unidades e os tipos do Koha que ainda não têm exemplar aqui).
3. Código e nome perdem os espaços das pontas. Código vazio, nome vazio, código com mais de 64 caracteres ou nome com mais de 256 é recusado (`CatalogRefusal::BadCodeName`, com a posição) e nada é gravado.
4. A resposta diz quantos códigos foram criados, quantos renomeados e quantos já estavam iguais.
5. Renomear não muda nenhum registro: os registros guardam o código. Por isso o `row_version` das Instâncias não sobe. O OPAC guarda páginas por versão do registro; quem renomeia com o servidor no ar (a tela, parte b) limpa essas páginas.

### `elzevir koha-codes --data-dir DIR --url URL --user USUÁRIO`

6. Entra na interface da equipe do Koha em `URL` como `USUÁRIO`, com a senha da variável `ELZEVIR_KOHA_PASSWORD` (nunca na linha de comando, que fica no histórico e na lista de processos). O login é o mesmo formulário da tela (`mainpage.pl`, `login_op=cud-login` com o token CSRF da página), então não depende de `RESTBasicAuth` nem de chave OAuth no Koha.
7. Se o Koha estiver atrás de um segundo usuário e senha HTTP (um proxy), `ELZEVIR_KOHA_BASIC_USER` e `ELZEVIR_KOHA_BASIC_PASSWORD` os informam.
8. Lê pela API REST, de uma vez (`_per_page=-1`):
   - `GET /api/v1/libraries`: `library_id` → nome (`name`);
   - `GET /api/v1/item_types`: `item_type_id` → `description`;
   - `GET /api/v1/authorised_value_categories/LOC/authorised_values`: `value` → `description`;
   - `GET /api/v1/authorised_value_categories/CCODE/authorised_values`: idem, para coleções.
9. A descrição vem como o Koha guarda, inclusive parênteses ("(Estantes gerais)"); quem quiser outro nome renomeia na tela.
10. Limites de entrada: cada resposta tem no máximo 4 MiB e 10.000 entradas; entrada sem código ou sem nome é pulada e contada; o resto é conferido pelo comando (item 3).
11. Grava tudo com um `catalog.name_codes` só. O relatório, na saída padrão, traz por tabela quantos foram lidos, criados, renomeados e iguais; entradas puladas vão para a saída de erro.
12. Saída **0** quando gravou; **1** quando o login falha, uma resposta não é JSON do formato esperado, a senha não foi dada ou o comando recusa. Nada é gravado nesses casos.
13. Como os outros papéis, trava a pasta de dados ([linha de comando](linha-de-comando.md), item 10): roda com o servidor parado.

### Tela (parte b)

14. `GET /equipe/admin/codigos` lista as quatro tabelas, cada linha com código e nome, para quem tem a permissão de catalogação (sem sessão: vai para o login; sem a permissão: 403). A página inicial da equipe tem o link para ela.
15. Cada linha é um formulário HTML comum, sem JavaScript: o nome se edita na própria linha e `Enter` (ou "Gravar") envia `POST /equipe/admin/codigos` com a tabela, o código, o nome e o token CSRF da sessão. O servidor grava com `catalog.name_codes` em nome de quem está na sessão (`audit_log`), limpa as páginas guardadas do OPAC (item 5) e redireciona de volta para a tabela, com o aviso "Gravado".
16. A última linha de cada tabela acrescenta um código novo com seu nome, pelo mesmo `POST`. Um código que já existe só troca de nome.
17. Código ou nome vazio ou longo demais (item 3) volta a página com 422 e a mensagem do Fluent. Sem sessão: 401; token errado ou sem a permissão: 403; tabela desconhecida: 400.
18. Apagar código fica de fora: exemplar pode apontar para ele.
19. Textos só via Fluent; os nomes são dados e aparecem como foram gravados.

## 2. Origem no Koha

- Unidades: `admin/branches.pl`, tabela `branches` (`branchcode`, `branchname`); API `Koha::REST::V1::Libraries`.
- Tipos de item: `admin/itemtypes.pl`, tabela `itemtypes` (`itemtype`, `description`); API `Koha::REST::V1::ItemTypes`.
- Localização e coleção: `admin/authorised_values.pl`, categorias `LOC` e `CCODE` (`authorised_value`, `lib`, `lib_opac`); API `Koha::REST::V1::AuthorisedValues`.
- Permissão no Koha para ler pela API: `catalogue`.

## 3. Paridade

`livre`: os nomes são apresentação; a exportação MARC continua com os códigos (`952 $a`, `$b`, `$c`, `$8`, `$y`).

Testes:
- `crates/elzevir-app/src/koha_codes.rs`: leitura das quatro respostas, com fixture das respostas reais do oráculo (2026-11-14) reduzidas aos campos usados; limites; entradas puladas.
- `crates/elzevir-store/tests/codes.rs`: criar, renomear e manter; recusa sem gravar nada.
- `crates/elzevir-web/tests/staff_catalog.rs` (`a_cataloguer_lists_and_renames_the_codes`): permissões, token, recusa e gravação pela tela.
- `crates/elzevir/tests/koha_codes.rs`: o binário contra um Koha simulado local (login com CSRF, as quatro rotas), gravando os nomes; senha ausente e login recusado saem com 1.
