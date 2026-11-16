# Encerramento explícito do store — base da A7

## Comportamento

`Store::shutdown(self)` consome o store, fecha o pool de leitura, encerra a
fila escritora e espera a conclusão dos comandos já aceitos. Na conexão da
própria escritora, executa `PRAGMA optimize` e `wal_checkpoint(TRUNCATE)`.
Não abre outra conexão de escrita e não perde respostas de comandos aceitos.

O encerramento retorna erro de SQLite, pânico da escritora ou
`StoreError::CheckpointBusy` se um leitor externo impedir o checkpoint.
Assim, o servidor poderá distinguir uma parada limpa de manutenção incompleta.
`Drop` mantém a limpeza de melhor esforço quando não há como retornar o erro.

## Origem e paridade

Não porta comportamento do Koha. Concretiza a parada com checkpoint da A7 do
roadmap operacional, preservando a escritora única do doc 04. É a base de
armazenamento; este PR não registra serviços nem muda sinais do processo.

## Verificação

- Comando enfileirado é confirmado antes do retorno; WAL fica ausente ou vazio.
- A base pode ser reaberta com o dado confirmado.
- Leitor externo com snapshot ativo produz erro explícito de checkpoint.
