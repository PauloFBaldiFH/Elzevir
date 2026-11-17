# Logs operacionais estruturados — A3.3

## Comportamento

1. `serve` inicia logs depois de adquirir a trava e abrir a pasta de dados.
   Grava uma linha JSON por evento em `logs/elzevir.DATA.jsonl`, com data UTC,
   rotação diária e retenção de no máximo sete arquivos do namespace reservado
   ao appender (prefixo `elzevir`, sufixo `jsonl`). Arquivos de outros nomes
   são preservados. A retenção ocorre na abertura e na rotação.
2. A API de emissão aceita apenas eventos conhecidos: `server.started`,
   `server.stopped`, `server.failed`, `indexer.committed`, `indexer.failed`,
   `indexer.panicked` e `runtime.cleanup_failed`. O contexto é `institution = local`
   (uma instituição por processo) e a identidade aleatória da execução. Início
   inclui PID; commits incluem apenas a contagem numérica de efeitos.
3. Ciclos vazios não geram eventos; falhas de indexação registram código e
   mensagem estáticos, mantendo a tentativa no ciclo seguinte. Não registram
   o erro bruto da dependência. Falhas após configurar a execução produzem
   `server.failed`; erros anteriores à criação do contexto continuam no terminal.
4. Somente o alvo exato `elzevir::ops` e níveis INFO/WARN/ERROR são habilitados.
   Não inclui spans nem aceita configuração de filtro pelo ambiente. URLs,
   consultas, headers, corpos, credenciais, leitores, caminhos de dados e
   mensagens brutas de dependências não entram nos eventos.
5. A fila tem capacidade de 1.024 linhas e aplica contrapressão ao produtor
   quando cheia, sem descartar eventos por saturação. Na parada normal, encerra
   e aguarda o indexador antes do evento final e da descarga da fila.
   `server.stopped` descreve a parada dos listeners/indexador; confirmação
   explícita do checkpoint no `serve` e prazos de serviço ficam para a A7.
6. `logs` precisa ser um diretório regular, sem link simbólico. Links no namespace
   de arquivos do appender são recusados antes da abertura. Falha ao configurar
   a gravação impede o início. As permissões dos logs são herdadas da pasta;
   `init` já cria a pasta de dados privada em Unix.

## Origem e escopo

Melhoria operacional sem paridade com Koha, baseada na A3 da análise do painel
fornecida por Paulo. `tracing`, `tracing-subscriber` e `tracing-appender` fornecem
os eventos, formato JSON e rotação; passam pelas verificações de licença do projeto.
A consulta `status` continua sem criar logs ou abrir SQLite. Esta entrega conclui
os recortes de saúde, estado e logs da A3; IPC administrativo e painel seguem depois.

Queda abrupta, disco cheio, erro de escrita ou esgotamento do prazo de descarga
podem impedir persistência e retenção; não se trata de um log de auditoria transacional.
Visualização/exportação, métricas e níveis configuráveis ficam para etapas próprias.

## Verificação

Testes cobrem JSON, contexto, retenção, arquivos alheios, isolamento de spans e
alvos, requisições com marcadores secretos, falha de inicialização, links e
parada com eventos finais descarregados em Unix.
