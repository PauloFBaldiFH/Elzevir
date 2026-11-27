# API REST do catálogo público e da conta do leitor (v0)

> Status: v0, tarefa 4.7. Implementada: 4.7a (itens 1 a 7, catálogo e contrato) e 4.7b (itens 8 a 17, conta do leitor com tokens). Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md), Fase 4, tarefa 4.7; [busca e registro](busca-e-registro.md); [conta do leitor](conta-do-leitor.md); [ocultar no OPAC](../catalogo/ocultar-no-opac.md). Referência: a `api/v1` do Koha (`api/v1/swagger/swagger.yaml`, `Koha/REST/V1/Auth.pm`). Critério da tarefa: **contrato OpenAPI publicado e testado**.

Um aplicativo, um site da prefeitura ou um painel de outra biblioteca lê o acervo público e, com um token do leitor, a conta dele, em JSON. A API não está na lista de paridade (CLAUDE.md, "Paridade com o Koha"): o desenho é próprio, em JSON do modelo BIBFRAME, e não copia os caminhos nem os objetos do Koha, que expõem colunas do `biblio` e etiquetas MARC (regra de ouro do [ARCHITECTURE_VISION](../../ARCHITECTURE_VISION.md)). Do Koha ficam as ideias: rotas públicas separadas das da equipe, contrato publicado e validado, um token por cliente.

## 1. Comportamento

### Geral

1. Todas as rotas ficam sob `/api/v1`, no mesmo endereço do OPAC, e respondem `application/json; charset=utf-8`. Nenhuma lê a sessão de cookie do OPAC nem da equipe.
2. O contrato é um documento **OpenAPI 3.0.3** escrito à mão e servido em `GET /api/v1/openapi.json`. Toda rota da API está nele, com os parâmetros e o esquema de cada resposta; rota fora do documento não existe.
3. Erro é sempre `{"error": "<código>"}`, com códigos fixos em inglês (a interface de quem consome traduz):
   - `not_found` (404): a rota, o registro ou o recurso não existe ou não é público;
   - `bad_request` (400): parâmetro ou corpo fora da regra;
   - `unauthorized` (401): sem token, token vencido ou cartão e senha recusados;
   - `locked` (429): o cartão está bloqueado por tentativas (item 13);
   - `refused` (409): a circulação recusou o pedido, com `reason` (item 17);
   - `unavailable` (503): o catálogo, o índice ou a circulação falharam.
   Rota desconhecida sob `/api/v1` dá `not_found` em JSON, não a página 404 do OPAC.
4. Datas são `AAAA-MM-DD` no fuso da biblioteca; valores em dinheiro são inteiros em centavos, como no núcleo (`Money`).
5. Sem CORS por enquanto: o navegador de outro domínio não lê a API. Um cliente de servidor ou um aplicativo lê.

### Catálogo público (4.7a)

6. `GET /api/v1/search?q=&page=`: a busca do OPAC ([busca e registro](busca-e-registro.md)) com os mesmos limites: consulta cortada em 256 caracteres, 20 resultados por página, página de 1 a 500, página que não é número lida como 1, consulta vazia dá lista vazia. Instâncias ocultas não aparecem. A resposta:
   - `query`, `total`, `page`, `has_next`;
   - `results`: para cada Instância, `instance` (número), `work`, `hub` (ou `null`), `title`, `contributor`, `date`, `call_number` (ou `null`), `content` e `carrier` (os nomes estáveis de `ContentKind` e `CarrierKind`, como `text` e `print`, ou `null`), `items` e `available` (contagens).
7. `GET /api/v1/instances/{id}`: o registro de uma Instância visível no OPAC; oculta ou inexistente dá `not_found`, e `id` que não é número também. A resposta traz o que a página do registro mostra, sem códigos MARC nem do Koha:
   - `instance`, `work`, `hub`, `title`, `work_title`, `other_titles`, `publisher`, `date`, `classification`, `summary`, `audience`, `genres`, `languages`, `content`, `carrier`;
   - `contributors` e `subjects`: listas de `{kind, name}`, com `kind` em `snake_case` (`person`, `organization`, `topic`, `place`…);
   - `identifiers` e `notes`: listas de `{kind, value}` (`isbn`, `issn`…; `general`, `contents`, `local`);
   - `relations`: lista de `{kind, work}` (`translation_of`, `part_of`…);
   - `items`: os exemplares, cada um com `branch` e `location` (o nome do cadastro, ou `null`), `call_number`, `available` e `status`, na ordem do OPAC (item 7 de busca-e-registro): `lost`, `on_loan`, `in_transit`, `on_hold_shelf`, `withdrawn`, `damaged`, `restricted`, `not_for_loan` ou `available`; com `due` (a data) quando está emprestado. Nunca o código de barras, o tombo nem quem tem o exemplar (LGPD).

### Conta do leitor com tokens (4.7b)

8. `POST /api/v1/patron/tokens` com corpo JSON `{"card": "…", "password": "…"}` (todo corpo da API vai até 4 KiB; corpo fora da regra dá `bad_request`) entra como o login do OPAC ([conta do leitor](conta-do-leitor.md), itens 5 a 8): mesma verificação Argon2 com hash isca, mesma contagem de recusas. Aceito, responde **201** com `{"token": "…", "expires_in": 1800}`.
9. O token é aleatório (256 bits, como o id da sessão do OPAC), guardado só em memória, apartado das sessões de cookie. Vence depois de **30 minutos sem uso** (item 9 da conta), e cada pedido com ele renova o prazo. Reiniciar o servidor apaga os tokens.
10. As rotas do leitor pedem o cabeçalho `Authorization: Bearer <token>`. Sem ele, ou com token desconhecido ou vencido, `unauthorized` com `WWW-Authenticate: Bearer`. Como o token não vai em cookie, não há CSRF.
11. `DELETE /api/v1/patron/tokens/current` encerra o token do pedido (**204**).
12. Respostas com dados do leitor levam `Cache-Control: no-store` (conta do leitor, item 11).
13. Cinco recusas de um cartão em 15 minutos bloqueiam o cartão por 15 minutos, contando juntas as do OPAC e as da API (o mesmo contador). Bloqueado, `locked`, sem verificar a senha.
14. `GET /api/v1/patron/account`: `name`, `card`, `branch`, `category`, `expires`, `restrictions` (`{kind, until}`, com `kind` `suspension` ou `overdues` e `until` `null` quando só a equipe levanta), `owed` (centavos) e `keeps_history`. Sem CPF nem endereço. Leitor removido depois de o token ser emitido: `unauthorized`, e o token é encerrado.
15. `GET /api/v1/patron/loans`: os empréstimos, o que vence primeiro antes (`item`, `title`, `call_number`, `branch`, `due` (o dia), `due_at` (o instante, UTC), `renewals`, `renewals_left`, `on_site`). `GET /api/v1/patron/debts`: as linhas da conta, a mais nova antes (`date`, `kind`, o código do tipo do Koha em minúsculas (`overdue`, `lost`, `payment`, `writeoff` e os outros de [conta.md](../circulacao/conta.md), item 27), `amount` positivo no débito e negativo no crédito, `outstanding`).
16. `GET /api/v1/patron/holds`: as reservas, a mais antiga antes (`hold`, `title`, `pickup` (o nome da unidade), `placed_on`, `status` `pending`, `in_transit` ou `waiting`, `position`, `expires_on`, `suspended` e `suspended_until`, o dia em que a suspensa volta ou `null`). `POST /api/v1/patron/holds` com `{"instance": n, "pickup": n}` reserva o registro como o OPAC ([conta do leitor](conta-do-leitor.md), item 21): **201** com `{hold, position}`; registro oculto ou inexistente dá `not_found`; recusa dá `refused` com `reason` entre `unavailable`, `pickup`, `restricted`, `debt`, `already_held`, `too_many` e `not_allowed`. `DELETE /api/v1/patron/holds/{id}` cancela uma reserva do próprio leitor ainda não separada (**204**); separada dá `refused` com `set_aside`; reserva de outro leitor ou já cancelada dá `not_found`.
16a. `PUT /api/v1/patron/holds/{id}/suspension` com `{}` ou `{"until": "AAAA-MM-DD"}` suspende uma reserva do próprio leitor ainda não separada, sem fim ou até o dia (**204**), como no OPAC ([conta do leitor](conta-do-leitor.md), item 25a); repetir troca o dia. `DELETE /api/v1/patron/holds/{id}/suspension` retoma (**204**). Separada dá `refused` com `set_aside`; reserva de outro leitor ou que não existe dá `not_found`; corpo ou dia ilegível dá `bad_request`. Com `patrons_suspend = false`, as duas dão `not_found`.
17. `POST /api/v1/patron/loans/{item}/renewal` renova um empréstimo do próprio leitor pelas regras de circulação do servidor ([renovação](../circulacao/renovacao.md)): **200** com o novo vencimento, ou `refused` com `reason` entre `on_site`, `too_many`, `overdue`, `owing`, `on_hold`, `too_soon`, `no_due_date`, `restricted` e `no_loan`. `{item}` é procurado entre os empréstimos do próprio leitor: Item que não está com ele dá `not_found`.

## 2. Casos de borda

- `GET /api/v1/instances/abc` e `/api/v1/instances/99999999999999999999`: `not_found`.
- Rota da API com método errado: **405** com `{"error": "method_not_allowed"}`.
- Sem as contas de leitor ligadas no servidor, as rotas de `/api/v1/patron` dão `not_found`, como `/conta`.

## 3. Testes

- 4.7a: o documento OpenAPI é JSON válido, versão 3.0.3, e cada caminho dele responde fora do 404 de rota; cada resposta de exemplo dos testes passa por um verificador de esquema mínimo (tipos, `required`, `properties`, `items`, `nullable`, `enum`, `$ref`) contra o esquema do documento; nenhuma rota `/api/v1` do roteador fica fora do documento.
- Busca com Instância oculta, registro oculto, exemplar emprestado (com `due`) e perdido, `id` inválido e rota desconhecida.
- 4.7b, de ponta a ponta no servidor real, com cada resposta passando pelo mesmo verificador de esquema: token emitido, usado e encerrado; sem token e token forjado; corpo inválido; conta sem CPF; débito da devolução atrasada; renovação aceita e recusada (`too_many`); Item de outro (`not_found`); a sessão do OPAC não aceita o token; reserva feita, repetida (`already_held`), listada e cancelada; bloqueio por tentativas da API que vale também no OPAC.
