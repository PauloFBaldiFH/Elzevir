# ADR 0005: serviço nativo e parada com resultado do checkpoint

- **Status:** adotado na A7, na continuidade autorizada do roadmap.
- **Spec:** [gestão do serviço](../specs/operacao/servico.md).

## Decisão

O mesmo binário registra e controla systemd no Linux e SCM no Windows. A CLI
oferece plano `--dry-run`; adaptadores permitem testes sem alterar serviços do
host. Instalação não inicia o processo, não cria contas nem altera permissões
de dados existentes. O instalador A9 fará o provisionamento dessas permissões.

No systemd, o processo usa conta não root e filesystem somente leitura, exceto
dados e `RuntimeDirectory` privado. `TMPDIR` aponta a esse runtime: os sockets
da A4 continuam visíveis à conta do serviço/root, sem o isolamento de caminho
que `PrivateTmp` causaria. No Windows, usa conta virtual exclusiva por serviço,
OWN_PROCESS, recuperação por reinício e a API segura de `windows-service`.
Unidades/serviços sem a marca esperada não são apropriados ou substituídos.

SIGTERM, Ctrl+C e STOP/SHUTDOWN do SCM acionam um controle durável comum. HTTP
drena por até 30 segundos; o gerenciador tem limite externo de parada. Após
encerrar listeners/indexador e aguardar leituras bloqueantes, o servidor consome
o store e pede manutenção à sua escritora. `Store::shutdown` retorna o resultado
do checkpoint; `Drop` mantém a limpeza de melhor esforço para os demais usos.
A trava da instituição permanece retida até o fechamento da base. Nenhuma
segunda conexão de escrita é aberta para desligar.

## Consequências

- Saúde do aplicativo e estado do gerenciador permanecem observações diferentes.
- Falha/ocupação do checkpoint não pode ser anunciada como parada limpa.
- Runtime do SO/SQLite bloqueado não pode ser cancelado com segurança; se o
  gerenciador matar o processo, SQLite recupera pelo WAL, sem promessa de limpeza.
- Instalação/desinstalação não apaga configuração, banco, índice ou logs.
- APIs do SCM foram verificadas por compilação cruzada; execução real do
  gerenciador exige runners Windows/systemd. OpenRC/launchd continuam previstos.
