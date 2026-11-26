# Fontes de autoridade (v0)

> Status: v0, tarefa 4.4. Base: [spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md) §17.1, [fontes de rascunho](fontes-de-rascunho.md), [cliente Z39.50](cliente-z3950.md), [estação de captura](estacao-de-captura.md) e [autoridades](autoridades.md). Critério da tarefa: **o nome autorizado e o identificador chegam ao bloco sem gravação automática**.

Quem liga um autor ou um assunto a uma Obra quer a forma autorizada de uma base de autoridades (Library of Congress, Biblioteca Nacional, VIAF, outra rede) em vez de digitar o nome de memória. Esta tarefa põe a busca de autoridades atrás da mesma infraestrutura das fontes de rascunho, como o Koha faz com os servidores Z39.50 de autoridade (`C4::Breeding::Z3950SearchAuth`): servidores Z39.50 ou SRU que devolvem registros MARC21 de autoridade, cadastrados por cada biblioteca.

A tarefa tem duas partes, cada uma num PR:

- **4.4a, busca:** configuração, consulta, leitura do registro de autoridade e a porta, com o `elzevir authority-search` (itens 1 a 15).
- **4.4b, tela:** a busca no painel "+ Autoridade", a aprovação e o identificador gravado com a ligação (itens 16 a 24).

## 1. Comportamento

### Configuração (4.4a)

1. Um servidor de autoridade é uma entrada de `[[draft_sources]]` com `records = "authority"`; sem a chave, ou com `records = "bibliographic"`, o servidor é bibliográfico, como antes. Vale para `kind = "sru"` e `kind = "z3950"`, com as mesmas chaves.
2. Servidores de autoridade não aparecem na estação de captura nem no `elzevir draft-search`; servidores bibliográficos não aparecem na busca de autoridades. Cada lista segue a ordem da configuração.
3. No SRU, o mapa `fields` de um servidor de autoridade usa as chaves `personal_name`, `corporate_name`, `meeting_name`, `name`, `subject`, `uniform_title` e `any`; as chaves bibliográficas (`isbn`, `issn`, `title`, `author`) num servidor de autoridade são erro, e as de autoridade num servidor bibliográfico também.
4. A origem de um rascunho de autoridade é `sru:<name>` ou `z3950:<name>`, como na busca bibliográfica.

### Consulta (4.4a)

5. A consulta tem o **nome** (texto, até 256 caracteres, não vazio) e a **classe** do painel de autoridades (pessoa, família, organização, evento, tópico, lugar, título, gênero/forma, período).
6. A classe escolhe o campo de busca do Koha (`C4::Breeding::_auth_build_query`):

   | Classe | Campo do Koha | Bib-1 | Chave de `fields` |
   |---|---|---|---|
   | pessoa | `authorpersonal` | `1=1` | `personal_name` |
   | organização | `authorcorp` | `1=2` | `corporate_name` |
   | evento | `authormeetingcon` | `1=3` | `meeting_name` |
   | família | `nameany` | `1=1002` | `name` |
   | título | `uniformtitle` | `1=6` | `uniform_title` |
   | tópico, lugar, gênero/forma, período | `subject` | `1=21` | `subject` |

7. No Z39.50, a consulta é um termo Type-1 com o atributo da tabela, em UTF-8, como na [busca bibliográfica](cliente-z3950.md) (item 7). No SRU, é `<índice>="<nome>"`, com o índice da chave da tabela; sem ela, o índice de `any`; sem os dois, o nome sem índice, com o mesmo escape da [busca bibliográfica](fontes-de-rascunho.md) (itens 8 e 9).
8. Página de 20 registros, até a página 50, como na busca bibliográfica.

### Registro de autoridade (4.4a)

9. Cada registro chega como MARC21 (MARCXML no SRU, ISO 2709 no Z39.50, com o conjunto de caracteres do item 21 do cliente Z39.50). O leitor de MARCXML aceita registros de autoridade só aqui; a importação continua a recusá-los. Um registro que não é de autoridade (líder 06 diferente de `z`) ou sem cabeçalho `1XX` é pulado e contado, como os registros ilegíveis.
10. O **cabeçalho** vem do primeiro `100`, `110`, `111`, `130`, `148`, `150`, `151` ou `155`, lido pela mesma regra do campo de assunto correspondente da importação (`600` a `655`, [mapeamento MARC21](mapeamento-marc21.md)): o nome com as suas partes (datas, numeração, título) ou o termo, e as subdivisões `$v`, `$x`, `$y` e `$z` depois de ` -- `. Assim o cabeçalho aprovado é igual ao que a importação gravaria.
11. A **classe** vem da etiqueta: `100` é pessoa (família com o primeiro indicador `3`), `110` organização, `111` evento, `130` título, `148` período, `150` tópico, `151` lugar, `155` gênero/forma.
12. As **datas** são o `$d` do `100`, quando houver.
13. O **identificador** é o número de controle na forma do `$0` do MARC21: `(<003>)<001>`, com os espaços do `001` mantidos; sem `003`, o `001` sozinho. Sem `001`, o registro não tem identificador, mas continua na lista.
14. A lista mostra também até três **formas variantes** (`400` a `455`), cada uma lida como o cabeçalho, para distinguir homônimos.
15. `elzevir authority-search --data-dir <pasta> --name <nome> --class <classe> [--page <n>]` consulta os servidores de autoridade e imprime, por servidor, o total e uma linha por registro (cabeçalho, classe, identificador e variantes), como o `draft-search`.

### Tela (4.4b)

16. Com pelo menos um servidor de autoridade habilitado, o painel "+ Autoridade" ([autoridades](autoridades.md), item 6) tem o botão **"Buscar nas fontes"**. Sem servidor, o botão não aparece e a rota do item 18 responde 404.
17. O botão consulta com o nome e a classe já digitados no painel; cada servidor é pedido num pedido próprio, todos ao mesmo tempo, e a resposta de cada um aparece quando chega, como na [estação de captura](estacao-de-captura.md) (itens 4 e 5): achados (com "Próxima página"), nada achado, sem resposta ou recusa.
18. `GET /equipe/acervo/fontes-de-autoridade/{n}?name=&class=&page=`, com a classe pelo nome do formulário (`person`, `genre_form`, `temporal`…), consulta só o servidor `n` (de 0, na ordem dos servidores de autoridade) e devolve o trecho HTML da resposta. Nome vazio ou longo demais, ou classe desconhecida, responde 422; servidor inexistente, 404. Pede sessão e a permissão `cataloguing`.
19. **"✓ Aprovar" não grava nada**: preenche o painel com a classe e o cabeçalho do registro e guarda o identificador num campo oculto da linha, mostrado como "De <servidor>: <identificador>". O servidor não guarda os resultados da busca: o cabeçalho e o identificador voltam do navegador como texto digitado e passam pelas mesmas regras do painel. Mudar o nome à mão depois de aprovar descarta o identificador, porque ele não descreve mais o nome digitado.
20. Ligar ([autoridades](autoridades.md), item 8) aceita o identificador aprovado e o grava como `$0` (registro de autoridade) da contribuição ou do assunto ligado, como os identificadores que o MARC traz ([autoridades](autoridades.md), item 16). O local de publicação não guarda identificador nesta tarefa, e o identificador enviado com ele é recusado (422, no campo). Um cabeçalho ligado com identificador segue a regra de identidade dos cabeçalhos com identificadores da migração 61: vira uma autoridade própria daquela ligação, em vez de reaproveitar outra de mesmo nome sem identificador.
21. O identificador tem no máximo 256 caracteres (acima disso, 422 no campo). É texto e entra no `$0` como veio; o bloco da autoridade o mostra como os outros identificadores ([autoridades](autoridades.md), item 16), que só viram link quando são endereços web.
22. O MARC exportado escreve o identificador como `$0` do `1XX`/`7XX` ou `6XX` da Obra ligada.
23. O registro de autoridade recebido não é guardado: só o cabeçalho, a classe e o identificador chegam ao painel. O Elzevir ainda não tem registros de autoridade próprios (fora desta tarefa).
24. O log da consulta segue a [estação de captura](estacao-de-captura.md), item 15, com o evento próprio das fontes de autoridade.

## 2. Origem no Koha

- Busca: `C4::Breeding::Z3950SearchAuth` e `_auth_build_query`, pelos servidores de `z3950servers` com `recordtype = 'authority'`; tela `cataloguing/z3950_auth_search.pl`.
- Aprovar: `ImportBreedingAuth` grava o registro no reservatório e o editor de autoridades abre com ele; nada vai para o catálogo antes de salvar.
- Diferenças intencionais:
  - a busca fica dentro do painel "+ Autoridade", sem janela;
  - o Elzevir não guarda o registro de autoridade (não tem registros de autoridade próprios), só o cabeçalho e o identificador;
  - a consulta usa um campo, escolhido pela classe; o Koha oferece todos os campos do formulário.

## 3. Paridade

`livre` (interface e configuração). A consulta segue os atributos do Koha. Testes com registros de autoridade gravados como fixture, nunca a rede.

## 4. Fora desta tarefa

- APIs próprias de cada base (id.loc.gov, VIAF, Wikidata) além de SRU e Z39.50.
- Registros de autoridade próprios do Elzevir e a importação de autoridades.
- Busca de autoridade no bloco de um agente ou assunto já gravado (substituir o nome ou acrescentar identificador).
