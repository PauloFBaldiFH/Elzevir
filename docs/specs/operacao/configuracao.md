# Configuração tipada do servidor — A1

## 1. Comportamento

1. `elzevir serve --config CAMINHO` lê explicitamente um arquivo TOML UTF-8.
   Não há busca automática de configuração nem interpolação de variáveis.
2. A seção `[server]` aceita `data_dir` (pasta da instituição), `listen`
   (IP e porta, incluindo IPv6 entre colchetes), `insecure_cookies` (booleano)
   e `iri_base` (base das IRIs BIBFRAME).
   O arquivo de exemplo é `elzevir.example.toml`.
3. Precedência: opções explícitas da CLI > arquivo > padrões internos.
   `listen` tem padrão `127.0.0.1:8080`; `insecure_cookies` é falso por padrão.
   `data_dir` precisa vir do arquivo ou da CLI. Esta etapa continua atendendo
   uma instituição por processo; a pasta identifica essa instituição.
4. `serve --data-dir PASTA [--listen IP:PORTA] [--insecure-cookies]` continua
   válido. Para sobrepor `insecure_cookies = true`, use
   `--insecure-cookies=false`; o sinalizador sem valor continua significando true.
5. Pasta relativa no arquivo é resolvida em relação à pasta desse arquivo.
   Pasta fornecida pela CLI é relativa ao diretório de trabalho. Não há
   expansão de `~` ou `$VAR`. A configuração é lida, nunca regravada.
6. Chaves/seções desconhecidas, tipos incorretos, endereços inválidos, pasta
   vazia, TOML inválido ou maior que 64 KiB são recusados antes de criar a
   pasta de dados ou abrir banco/índice. O arquivo deve ser válido mesmo que
   alguma opção seja sobreposta pela CLI. Os erros não reproduzem o conteúdo
   do arquivo: indicam caminho e, quando disponível, linha e coluna.
7. O endereço anunciado na inicialização é o efetivamente vinculado pelo
   listener, inclusive quando a porta configurada é zero (alocação pelo SO).
8. `--iri-base` continua disponível e tem precedência sobre `server.iri_base`.
   Quando ambos estão ausentes, mantém a base derivada da requisição.
9. A configuração operacional explícita não substitui o arquivo de domínio
   `DATA_DIR/elzevir.toml`: sua seção `[call_number]` e a tabela de autores
   continuam sendo carregadas pela rotina existente. São arquivos distintos;
   esta etapa não unifica os dois esquemas.

## 2. Origem no Koha

Não porta comportamento do Koha. Base: doc 04, §5 e §7, doc 06 e etapa A1
da análise do painel fornecida por Paulo em 2026-11-16.

## 3. Paridade

Livre. `serde` faz a desserialização tipada; `toml` (MIT/Apache-2.0) faz a
leitura do formato. Nenhuma regra bibliográfica ou de circulação muda.

## 4. Casos de borda

- Um arquivo inicia o catálogo e aplica a política de cookies.
- Opções explícitas da CLI vencem as do arquivo, incluindo false.
- O modo antigo inicia sem arquivo e com cookies Secure por padrão.
- Pasta relativa ao arquivo não depende do diretório de trabalho.
- Arquivo ausente, grande demais, não UTF-8, chaves desconhecidas e valores
  inválidos falham sem criar dados e sem mostrar valores do arquivo.
- Arquivo sem `data_dir` pode ser completado pela CLI; pasta vazia é inválida.
- IPv6 e padrões são cobertos na resolução tipada.

## 5. Desvios

Nenhum: não há área de paridade nesta entrega.

## 6. Fora do escopo

Inicialização de uma instituição, separação dos endereços da equipe,
`public_url`, `[backup]`, `[tunnel]`, reload, segredos, HTTPS embutido e serviço
ficam para etapas próprias. Chaves sem implementação são recusadas.
