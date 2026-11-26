# Fontes de rascunho: porta comum e cliente SRU (v0)

> Status: v0, tarefa 4.1. Base: [spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md) §17.1, decisão de [fontes de rascunho](../../arquitetura/04-decisoes.md) do doc 04, [importação](importacao.md) e [casamento](casamento.md). Critério da tarefa: **a busca por ISBN, título e autor devolve rascunhos de cada servidor configurado, com a origem; servidor fora do ar aparece como "sem resposta"**.

Quem cataloga um livro que a biblioteca ainda não tem quer partir do registro de outra biblioteca (Biblioteca Nacional, Library of Congress, outra rede) em vez de digitar tudo. Esta tarefa cria a porta pela qual essas fontes entram e o primeiro cliente, SRU. A tela (estação de captura) e a aprovação são a tarefa 4.2; Z39.50 é a 4.3.

## 1. Comportamento

### Porta

1. A camada de aplicação define a porta `DraftSource`. Ela recebe uma consulta e uma página e devolve, para cada servidor, um resultado:
   - **achados**: o total que o servidor diz ter e os rascunhos da página;
   - **sem resposta**: o servidor não respondeu dentro do tempo-limite ou a conexão falhou;
   - **recusa**: o servidor respondeu com um diagnóstico (consulta não suportada, índice desconhecido), com o texto do servidor.
2. Um rascunho é um registro MARC21 lido pelo leitor de MARCXML do Elzevir (com os limites dele) e a sua origem, `sru:<nome do servidor>`. O rascunho não é gravado em lugar nenhum nesta tarefa: vive só na resposta.
3. Para a lista de resultados, cada rascunho traz um resumo tirado do MARC pelo mesmo mapeamento da importação, sem os campos locais `9XX` da biblioteca de origem (um `952` incompleto faria o mapeamento recusar o registro): título, responsabilidade, edição, publicação (lugar, editora, data) e ISBNs.
4. Os servidores são consultados **em paralelo**, cada um com o seu tempo-limite. Um servidor lento não segura os outros: a porta devolve quando todos responderam ou estouraram o prazo.
5. A ordem dos resultados é a ordem dos servidores na configuração.

### Consulta

6. A consulta tem os campos `isbn`, `issn`, `title`, `author` e `any` (qualquer campo). Pelo menos um campo preenchido; campo vazio ou só de espaços não conta. Cada valor tem no máximo 256 caracteres.
7. O ISBN é normalizado como no [bipe](bipe.md): sem hifens nem espaços. Um ISBN com dígito verificador errado é enviado como digitado, porque a fonte pode tê-lo registrado assim.
8. A consulta CQL segue a regra do Koha (`C4::Breeding::_build_query` e `_translate_query`):
   1. os campos preenchidos, em ordem alfabética do nome, viram `<índice>="<valor>"`, ligados por ` and `;
   2. o índice de cada campo vem do mapa `fields` do servidor (por exemplo, `isbn = "bath.isbn"`);
   3. um campo sem índice no mapa usa o índice de `any`; sem índice de `any`, o campo vai sem índice (`"<valor>"`, busca em qualquer lugar para o CQL).
9. Diferença intencional: dentro do valor, `"` e `\` são escapados (`\"` e `\\`), para que o texto digitado nunca mude a forma da consulta. O Koha não escapa.
10. Página de 20 registros, como o Koha (`hits_per_page`). A página `n` pede `startRecord = (n - 1) × 20 + 1`. Página máxima: 50.

### Pedido e resposta SRU

11. O pedido é `GET <url>?operation=searchRetrieve&version=<versão>&query=<CQL>&startRecord=<n>&maximumRecords=20&recordSchema=<esquema>`, com os valores codificados para URL. Versão padrão `1.2` (a `2.0` também é aceita); esquema padrão `marcxml`. O registro vem como XML, não como texto escapado: `recordPacking=xml` na 1.2 e `recordXMLEscaping=xml` na 2.0.
12. Só `http` e `https`. A URL do servidor vem da configuração, nunca do navegador.
13. A resposta é lida com limites: no máximo 8 MiB de corpo; no máximo 20 registros aceitos por resposta; cada registro passa pelo leitor de MARCXML com os limites dele.
14. Da resposta saem `numberOfRecords`, os `recordData` com o registro MARC21 (`marc:record` ou `record` sem prefixo) e os `diagnostics`. Diagnóstico vira **recusa** com a mensagem do servidor (até 300 caracteres). Registro que não é MARC21 válido é pulado e contado; os outros seguem.
15. HTTP fora de 2xx vira **recusa** com o código; falha de rede ou tempo estourado vira **sem resposta**.

### Configuração

16. Os servidores ficam no `elzevir.toml`, numa lista `[[draft_sources]]`, em ordem de prioridade:

    ```toml
    [[draft_sources]]
    name = "Biblioteca de exemplo"
    kind = "sru"
    url = "https://sru.example.org/catalogo"
    version = "1.2"            # opcional, 1.2 ou 2.0
    record_schema = "marcxml"  # opcional
    timeout_seconds = 10       # opcional, de 1 a 60
    enabled = true             # opcional
    [draft_sources.fields]
    isbn = "bath.isbn"
    title = "dc.title"
    author = "dc.creator"
    any = "cql.anywhere"
    ```

17. `name` é obrigatório, único e tem até 80 caracteres; é o que aparece na origem (`sru:<name>`). `kind` aceita `sru` e `z3950` ([cliente Z39.50](cliente-z3950.md), itens 15 e 16); uma chave de um tipo num servidor do outro é erro. Configuração inválida impede o servidor de subir, com a mensagem do campo errado, como no resto do `elzevir.toml`.
18. Sem nenhum servidor configurado (ou todos com `enabled = false`), a porta não existe e a tela não oferece a estação.
19. A configuração não traz servidores de exemplo de nenhuma instituição: cada biblioteca cadastra os seus.

### Privacidade e registro

20. A consulta vai só do servidor do Elzevir à fonte. O navegador nunca fala com a fonte.
21. A consulta leva só os termos bibliográficos digitados: nunca dado de leitor nem da sessão.
22. Quando a tela consulta as fontes, o log registra a posição do servidor na configuração, o resultado (achados, sem resposta, recusa) e o tempo, sem os termos da consulta nem o nome do servidor ([estação de captura](estacao-de-captura.md), item 15).

### Diagnóstico pela linha de comando

23. `elzevir draft-search --data-dir <dir> [--isbn] [--issn] [--title] [--author] [--any] [--page]` consulta os servidores habilitados do `elzevir.toml` e imprime, por servidor, o total e uma linha por rascunho (título, autor, edição, publicação e ISBNs), "no response" ou "refused" com a mensagem. Serve para a biblioteca conferir os servidores antes de catalogar. Sem servidor configurado, o comando termina com erro e diz isso.
24. O transporte HTTP usa os certificados do sistema e aceita `http` só porque a biblioteca o configurou; segue até 3 redirecionamentos e lê no máximo 8 MiB.

## 2. Origem no Koha

- Busca nas fontes: `C4::Breeding::Z3950Search` (as duas, Z39.50 e SRU, pelo ZOOM), com `_bib_build_query`, `_build_query`, `_translate_query` e `_create_connection`.
- Cadastro dos servidores: tabela `z3950servers` (`servername`, `host`, `port`, `db`, `servertype` `zed`/`sru`, `sru_fields`, `sru_options`, `syntax`, `encoding`, `timeout`, `rank`, `checked`) e `admin/z3950servers.pl`.
- Diferenças intencionais: configuração no `elzevir.toml` em vez de tabela (decisão do doc 04 sobre configuração tipada); escape do valor na CQL (item 9); nenhuma transformação XSLT por servidor (`add_xslt`), porque o rascunho passa pelo mesmo mapeamento da importação.

## 3. Paridade

Não é área de paridade (a paridade de Z39.50 do CLAUDE.md vale para o servidor, tarefa 4.6). A tradução da consulta segue o Koha por coerência; os testes usam respostas SRU gravadas em fixture e nunca a rede.

## 4. Fora desta tarefa

- Estação de captura, prévia e aprovação: tarefa 4.2.
- Z39.50: tarefa 4.3.
- Fontes de autoridade: tarefa 4.4.
- Cadastro dos servidores pela tela.
