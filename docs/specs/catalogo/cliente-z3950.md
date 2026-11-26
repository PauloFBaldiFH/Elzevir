# Cliente Z39.50 das fontes de rascunho (v0)

> Status: v0, tarefa 4.3. Base: [fontes de rascunho](fontes-de-rascunho.md) (porta, resultado por servidor, página de 20), [estação de captura](estacao-de-captura.md), Z39.50-2003 (ANSI/NISO Z39.50, ASN.1 do anexo ASN1) e o conjunto de atributos Bib-1. Critério da tarefa: **os mesmos rascunhos da 4.1 vêm de servidores Z39.50, gravados como fixture; o decodificador tem alvo de fuzzing**.

Muitas bibliotecas (Biblioteca Nacional, redes Pergamum, outros Koha) só oferecem Z39.50. Esta tarefa põe um cliente Z39.50 atrás da mesma porta `DraftSource` da 4.1: a estação de captura (4.2) e o `elzevir draft-search` passam a listar servidores Z39.50 e SRU juntos, na ordem da configuração.

A tarefa tem duas partes, cada uma num PR:

- **4.3a, protocolo:** BER e as mensagens Init, Search, Present e Close, sem rede (itens 1 a 14).
- **4.3b, cliente:** a conversa com o servidor, a configuração, os conjuntos de caracteres e a ligação com a porta (itens 15 a 27).

## 1. Comportamento

### BER (4.3a)

1. As mensagens são codificadas e lidas em BER (X.690) por código próprio, sem dependência nova.
2. A leitura recusa, sem entrar em pânico e sem alocar além do que recebeu:
   - comprimento indefinido (o Z39.50 dos servidores usuais usa sempre o definido);
   - comprimento maior que o que resta da mensagem, ou maior que 8 MiB (o mesmo limite do SRU);
   - aninhamento com mais de 32 níveis;
   - número de etiqueta com mais de 4 bytes e inteiro com mais de 8 bytes.
3. Um elemento desconhecido dentro de uma mensagem conhecida é pulado (extensões do servidor não quebram a leitura).

### Mensagens (4.3a)

4. **Init**: versões 2 e 3, opções `search` e `present`, tamanho preferido de mensagem e tamanho excepcional de registro de 8 MiB, nome da implementação `Elzevir` e a versão do programa. Sem autenticação nesta tarefa.
5. **Resposta ao Init**: o resultado (aceito ou recusado) e, se houver, a mensagem do servidor.
6. **Search**: base(s) da configuração, conjunto de resultados `default`, `smallSetUpperBound = 0`, `largeSetLowerBound = 1`, `mediumSetPresentNumber = 0` (nenhum registro volta na busca; eles vêm no Present) e a consulta Type-1 (RPN) com o conjunto de atributos Bib-1.
7. **Consulta Type-1**, pela regra do Koha (`C4::Breeding::_bib_build_query` e `_build_query`):
   1. os campos preenchidos, na ordem alfabética dos nomes do Koha (`author`, `isbn`, `issn`, `srchany`, `title`; `any` do Elzevir é o `srchany` do Koha);
   2. cada um com os atributos do Koha: ISBN `1=7 5=1`, ISSN `1=8 5=1`, título `1=4`, autor `1=1003`, qualquer campo `1=1016`;
   3. os termos ligados por `and` à esquerda, como o PQF `@and @and t1 t2 t3` do Koha: `(t1 and t2) and t3`;
   4. o termo vai como `general` (cadeia de bytes) em UTF-8, com o ISBN já normalizado ([fontes de rascunho](fontes-de-rascunho.md), item 7).
8. **Resposta ao Search**: total de registros, situação da busca e, se houver, o diagnóstico (número da condição Bib-1 e texto adicional).
9. **Present**: conjunto `default`, primeiro registro `(página - 1) × 20 + 1`, até 20 registros, sintaxe preferida USMARC/MARC21 (`1.2.840.10003.5.10`) e conjunto de elementos `F` (completo, como o Koha).
10. **Resposta ao Present**: os registros, cada um como os bytes ISO 2709 que vieram no `EXTERNAL` (`octet-aligned`) com o OID da sintaxe; um registro substituído por diagnóstico (`surrogateDiagnostic`) ou em outra sintaxe é contado como ilegível; um diagnóstico no lugar de todos os registros vira recusa.
11. **Close**: motivo `finished`, enviado ao terminar, sem esperar resposta.
12. Mensagem que não é a esperada naquele passo (por exemplo, `Close` do servidor no lugar da resposta ao Search) é recusa, com o motivo de fechamento quando houver.
13. O texto de um diagnóstico é `<condição>: <texto adicional>` (ou só a condição), cortado em 300 caracteres, como na 4.1.
14. O leitor de mensagens tem alvo de fuzzing (`z3950_pdu`), e os testes usam mensagens montadas byte a byte a partir do ASN.1 da norma, nunca a rede.

### Cliente (4.3b)

15. Configuração no `elzevir.toml`, na mesma lista `[[draft_sources]]`:

    ```toml
    [[draft_sources]]
    name = "Biblioteca de exemplo"
    kind = "z3950"
    host = "z3950.example.org"
    port = 210                 # opcional
    database = "catalogo"      # uma ou mais, separadas por vírgula
    encoding = "auto"          # opcional: auto, utf-8 ou marc-8
    timeout_seconds = 10       # opcional, de 1 a 60
    enabled = true             # opcional
    ```

16. `name`, `timeout_seconds` e `enabled` valem como no SRU; `host` é obrigatório (nome ou endereço, sem esquema nem porta); `port` de 1 a 65535; `database` obrigatório, cada nome com até 64 caracteres.
17. A origem de um rascunho é `z3950:<name>`.
18. A conversa é: conectar, Init, Search e, se o total alcança a página, Present; depois Close. O tempo-limite do servidor vale para a conversa inteira, não para cada passo.
19. Conexão recusada, nome que não resolve ou tempo estourado: **sem resposta**. Init recusado: **recusa** "init refused" (com a mensagem do servidor, se houver). Diagnóstico na busca ou no Present: **recusa** com o texto do item 13.
20. Página além do total: **achados** com o total e nenhum rascunho, sem Present.
21. Conjunto de caracteres de cada registro, como o Koha (`C4::Charset::MarcToUTF8Record` para MARC21): com `encoding = "auto"`, posição 09 do líder `a` é UTF-8 e o resto é MARC-8; `utf-8` e `marc-8` forçam a leitura. O registro passa a UTF-8 (líder 09 = `a`) antes do resumo e do mapeamento.
22. Registro que não é ISO 2709 válido, ou que não se converte, é pulado e contado, como no SRU.
23. A rede fica fora da crate `elzevir-sources`: ela recebe uma porta de conexão (abrir um fluxo de bytes com prazo), e o binário dá a conexão TCP.
24. A conexão TCP usa o prazo restante da conversa em cada leitura e escrita e não usa TLS (o Z39.50 das bibliotecas é TCP simples).
25. `elzevir draft-search` e a estação de captura passam a ter servidores Z39.50 e SRU na mesma lista, na ordem da configuração.
26. Teste de ponta a ponta do comando com um servidor Z39.50 de mentira numa porta local, que responde com as mensagens gravadas.
27. O log da estação ([estação de captura](estacao-de-captura.md), item 15) vale igual para Z39.50.

## 2. Origem no Koha

- Busca: `C4::Breeding::Z3950Search`, `_bib_build_query`, `_build_query` e `_create_connection` (ZOOM com `elementSetName = F`, `preferredRecordSyntax`, `databaseName`, `user`, `password`).
- Conjunto de caracteres: `C4::Breeding::_handle_one_result` e `C4::Charset::MarcToUTF8Record`.
- Cadastro: tabela `z3950servers` (`host`, `port`, `db`, `syntax`, `encoding`, `timeout`, `userid`, `password`, `attributes`).
- Diferenças intencionais: sem os atributos extras por servidor (`attributes`), sem usuário e senha e sem XSLT por servidor nesta tarefa; a configuração fica no `elzevir.toml`.

## 3. Paridade

O CLAUDE.md põe o Z39.50 na lista de paridade; a paridade vale para o **servidor** (tarefa 4.6), que responde a clientes de outras bibliotecas. Este cliente segue o Koha na consulta e no conjunto de caracteres, e os testes não usam a rede.

## 4. Fora desta tarefa

- Autenticação (`idAuthentication`), atributos extras por servidor, outras sintaxes (UNIMARC, XML) e `Scan`.
- Conjuntos de resultados nomeados e várias buscas na mesma conexão.
