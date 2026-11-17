# Gestão do serviço — A7

## 1. Comportamento

1. `elzevir service install --config ARQUIVO [--name elzevir-NOME]` registra
   uma instituição já inicializada. `--dry-run` apenas imprime um plano JSON
   revisável; não registra serviço, cria usuário ou altera arquivos do sistema.
   O binário/configuração/dados devem existir; a configuração deve ser arquivo
   regular sem link simbólico, para não mudar a base de caminhos relativos ao
   registrar o serviço. Caminhos são absolutos e nomes
   são `elzevir` ou `elzevir-` seguido de letras minúsculas, dígitos e hífens,
   com limite total de 64 caracteres. Não interpreta shell nem aceita controles.
2. `service start|stop|restart|uninstall --name NOME [--dry-run]` opera somente
   serviços identificados como gerenciados pelo Elzevir. `service status --name
   NOME` imprime JSON de estado do gerenciador, distinto da saúde da A3. Não
   há instalação/inicialização automática durante testes ou uso do painel.
3. Linux/systemd: unidade em `/etc/systemd/system/NOME.service`, `Type=simple`,
   reinício em falha, SIGTERM, limite externo de parada de 45 s, `User=elzevir`
   por padrão (`--account` pode escolher outra conta não root já existente),
   `UMask=0077`, `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome=true`
   e somente a pasta da instituição gravável. Portas fixas abaixo de 1024
   habilitam apenas `CAP_NET_BIND_SERVICE`. ExecStart passa config absoluta;
   caminhos com espaço, $, % e aspas usam os escapes do systemd, não shell.
4. Instalar no systemd publica a unidade sem sobrescrita, recarrega o gerenciador
   e habilita início no boot; não inicia imediatamente. Falha tenta desfazer
   somente o que a instalação criou. Desinstalar para/desabilita e remove só
   a unidade reconhecida; preserva configuração, banco, índice e logs. Comandos
   externos usam argumentos separados, execução limitada e saída limitada.
5. Windows: serviço nativo OWN_PROCESS, início automático, conta virtual
   `NT SERVICE\\NOME`, marca no nome de exibição/descrição e recuperação por reinício em
   falha. Não usa WSL, tarefa agendada nem wrapper de shell. `service run`
   é entrada interna do SCM, não substitui `serve` no terminal. Gerenciador
   existente sem o nome de exibição esperado, tipo OWN_PROCESS e a conta virtual
   correspondente é preservado. Desinstalação pede parada,
   espera estado parado e apaga só o registro do serviço.
6. Conta/pastas/permissões devem ser provisionadas antes de iniciar: no Linux
   a conta precisa ler o executável/config e gravar a pasta de dados, fora de
   `/home` e `/root` por causa de ProtectHome. No Windows, o SID virtual precisa
   de RX no executável, R na configuração e modificação nos dados. A7 não
   reescreve ACLs, não troca dono de dados existentes e não cria contas; isso
   pertence ao instalador A9. Operações de registro exigem privilégios do SO.
7. A parada do processo usa o mesmo controle para Ctrl+C/SIGTERM no Unix e
   STOP/SHUTDOWN do SCM no Windows. Handlers são instalados antes do anúncio de
   prontidão. O SCM recebe StartPending, Running só após listeners/metadata,
   StopPending e Stopped apenas depois da limpeza. Falhas retornam exit code.
8. Ao parar, deixa de aceitar conexões, encerra IPC, aguarda HTTP em andamento
   por até 30 s e cancela conexões restantes. Para o indexador, drena a fila
   escritora e executa `optimize`/`wal_checkpoint(TRUNCATE)` na própria conexão
   escritora, sem abrir outra escrita. Checkpoint ocupado/erro é falha explícita.
   Metadata/socket são removidos; a trava de dados é liberada só após fechar
   a base. Logs operacionais são descarregados. Nenhum checkpoint é feito pela UI.
9. Timeout HTTP não cancela I/O já bloqueado no SO/SQLite. O processo mantém a
   trava enquanto espera esses trabalhos; o limite externo do gerenciador pode
   terminar a tarefa à força. Nesse caso valem as garantias de recuperação WAL,
   sem afirmar parada limpa. O status do serviço não substitui `elzevir status`.

## 2. Origem no Koha

Não porta comportamento do Koha. Base: A7 da análise de Paulo e doc 04 §6.

## 3. Paridade e arquitetura

Livre. Gerenciamento fica na borda do binário, com adaptadores testáveis sem
gerenciador real. `windows-service` 0.8.1 (MIT/Apache-2.0) fornece SCM via API
segura. O núcleo e os comandos de negócio não mudam; o shutdown explícito do
store apenas expõe o resultado da manutenção pela escritora já existente.

## 4. Casos de borda

- Plano não cria arquivos; nome inválido/config inválida falham antes de ações.
- Unidade/serviço existente, links ou marca desconhecida não são substituídos.
- Conta root recusada; caminhos com metacaracteres preservados como argumentos.
- Fila de comandos é concluída antes do checkpoint; erro não vira sucesso.
- SIGTERM libera listeners, metadata, socket e trava; base pode ser reaberta.
- Controle STOP antes de Running não perde a solicitação; saúde continua separada.
- Adaptadores testados com dublês; SIGTERM com processo temporário Linux real.

## 5. Desvios

Nenhum: não há área de paridade.

## 6. Limites desta entrega

A distribuição inclui adaptadores systemd, OpenRC e SCM Windows. Plataformas
sem adaptador, inclusive macOS/launchd, recusam operações. Instaladores, ACLs,
atualizações e reinício pelo painel são descritos nas specs de distribuição,
ferramentas e operações do painel. Esta máquina não tem systemd como PID 1 nem
SCM: testes reais de registro/início exigem runners dessas plataformas.
Compilação cruzada e testes com dublês não comprovam instalação nativa.

## 7. OpenRC na distribuição A9

Linux com OpenRC usa script próprio em `/etc/init.d`, com marca de propriedade,
conta não-root e `start-stop-daemon` em funções com argumentos citados
individualmente. Não passa caminhos do operador por `eval`. SIGTERM tem janela
de 45 segundos antes da terminação forçada. Registro usa `rc-update`; controle
usa `rc-service`, com limites idênticos aos de systemd. Conta e permissões são
provisionadas pelo instalador. Portas abaixo de 1024 são recusadas neste
adaptador; use porta não privilegiada e proxy/túnel. O plano não executa nada.
O adaptador é selecionado quando OpenRC está presente e systemd não está ativo.
