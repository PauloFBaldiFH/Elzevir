# Saúde local do servidor — A3.1

## Comportamento

1. `serve` abre um listener exclusivo em `127.0.0.1`, com porta atribuída pelo
   sistema. Anuncia o endereço efetivo como `elzevir: health on http://IP:PORT/health`.
   Apenas esse listener oferece `GET /health`; a rota continua ausente no catálogo.
2. A resposta JSON tem `schema = 1`, `instance` (32 dígitos hexadecimais
   aleatórios por execução), `pid`, `version`, `uptime_seconds`, `database`
   e `indexer`. Não inclui caminhos, configuração, credenciais ou dados de usuários.
   Retorna 200 somente quando banco e indexador estão `ok`; nos demais estados,
   503. As respostas têm `Cache-Control: no-store`.
3. O banco é sondado por uma consulta pequena ao esquema (`staff_accounts`,
   `audit_log`, `effect_outbox`), usando o pool de leitura existente. A consulta
   roda fora das threads assíncronas. Não abre outra conexão, migra dados,
   executa `quick_check` ou testa escrita.
4. No máximo quatro sondas executam simultaneamente. Saturação responde como
   não pronto; a permissão permanece com o trabalho até terminar, inclusive
   se a requisição for cancelada.
5. O indexador começa em `starting`, registra `ok` após um ciclo concluído e
   `failed` após falha. Observação com mais de cinco segundos também é `failed`.
   A falta de uma conclusão recente não é confundida com prontidão.
6. Falha ao abrir o listener local impede o início do servidor. Ctrl+C encerra
   os dois listeners pelo mesmo sinal de parada. O catálogo mantém as opções
   existentes de IRI, cookies e tabela de autores.

## Origem e escopo

Melhoria operacional sem paridade com Koha, conforme observabilidade dos
documentos de arquitetura e A3 da análise do painel fornecida por Paulo.
`getrandom`, já presente no workspace, gera a identidade da execução; axum,
serde e tokio existentes fornecem HTTP, JSON e coordenação.

Este é o primeiro recorte da A3. Descoberta por arquivo local, `elzevir status`,
logs estruturados, IPC administrativo, diagnóstico integral e sinais de serviço
ficam para recortes posteriores. A prontidão descreve o instante observado;
não garante disponibilidade futura nem integridade completa do banco.

## Verificação

Testes cobrem início sem ciclo, falha e observação antiga do indexador,
pressão sobre as sondas, contrato HTTP/JSON, bind exclusivo em loopback,
isolamento da rota pública e parada conjunta por Ctrl+C em Unix.
