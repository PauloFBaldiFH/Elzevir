# Marco C — agendas e conexões de nuvem

Primeiro incremento da proposta consolidada. O PR #275 integrou o Marco B na main em `0b509cd9`. A branch de trabalho é `codex/painel-marco-c`. O commit do estudo é `9cfb0625`, a central de agendas e a navegação da nuvem estão em `0bf0c92d`, a correção do catálogo em `47285011` e o ajuste da leitura em terminal compacto em `71b0e0b9`. [Estudo e matriz de prioridades](../../evolucao-painel-operacional.md) · [Contrato do painel](../../specs/operacao/painel-operacoes.md) · [Contrato da nuvem](../../specs/operacao/nuvem.md)

## O que o operador consegue fazer

- Abrir **Agendamentos** para consultar a agenda local e os planos externos. Na política local, conferir destino, quantidade, horário e fuso. Em tela larga, a atividade recente agrupa os recibos de backup do servidor. A edição parte da política observada. Se a observação faltar ou mudar antes de confirmar, o painel solicita nova leitura.
- Selecionar um plano e revisar o destino antes de executar pelo worker existente. A seleção sobrevive à reordenação da lista. Mudar a conexão ou a pasta invalida a seleção. As consultas e os cancelamentos deixam a fila intacta.
- Navegar por Conexões, Planos, Na nuvem e Histórico usando clique ou Tab/Enter. Consultar provedores, filtrar o catálogo por nome ou descrição, abrir a configuração com o tipo preenchido, responder à pergunta do provedor e revisar operações. O assistente preserva as validações de sessão, opções privadas do rclone e o canal administrativo existente.
- Ler a falha de descoberta no histórico. Ações que dependem de serviço disponível têm proteção também no teclado. As descrições longas do painel de nuvem quebram em linhas e o paginador usa a quantidade de linhas efetivamente desenhadas.

## Antes e depois no executável

| Percurso | Evidência e condições |
| --- | --- |
| Marco B, antes da mudança | [Agendamentos abria um formulário](antes-agendamentos.png) · [Área anterior de nuvem](antes-nuvem.png) |
| Consulta de agendas | [160×45](agendamentos-160x45.png) · [80×24](agendamentos-80x24.png). Política gravada pelo painel e relida pela CLI do servidor. |
| Central de nuvem | [160×45](nuvem-160x45.png) · [80×24](nuvem-80x24.png) · [Falha registrada ao consultar os provedores](nuvem-falha-consulta.png). A instalação de ensaio começa sem conexões. |
| Nova conexão | [Busca no catálogo real em cache](provedor-filtro.png) · [Configuração com o provedor escolhido](provedor-revisao.png). As imagens mostram seleção e revisão. Nenhuma conta Google foi autenticada. |
| Comparação com a intenção visual | [Propostas 05–07 ao lado das telas executáveis](comparacao.png). Em 80 colunas a agenda local fica acima da lista de planos. O catálogo tem filtro e foco por linha. |

## Operações exercitadas

1. **Binário e serviço em PTY Linux.** Uma instituição exclusiva foi inicializada, o serviço escutou em loopback, e o teste enviou cliques SGR de pressionar e soltar. Abrir a página e cancelar o formulário preservou a política original. A edição definiu 23h59 em America/Sao_Paulo e outra conexão à CLI leu os dois valores. Foram verificados 80×24, 100×30, 120×30, 160×45, 180×50 e 200×60. Depois da parada, o clique e o atalho recusaram editar uma política desconhecida.
2. **Falha do serviço e leitura de cache.** A consulta real do catálogo gerou um trabalho do servidor. O instalador oficial do rclone gerenciado também foi exercitado em instituição descartável. O recibo foi de falha e a ferramenta continuou indisponível em seu diretório protegido. Para exercitar separadamente o caso de uma instalação com catálogo prévio, o ensaio carregou apenas a saída pública de `rclone --config /dev/null config providers` no cache da instituição temporária. A pesquisa, o clique na linha do Drive e o cancelamento rodaram na TUI. A fila foi conferida antes e depois.
3. **Rclone real em teste de integração.** No Linux, o teste opcional usa o executável de distribuição em `/usr/bin/rclone` e os mesmos métodos de descoberta, configuração, resposta e teste do worker. O catálogo de 46 provedores dessa versão inclui nomes como `google cloud storage`. O teste inicialmente falhou porque o Elzevir recusava a lista inteira. Com a correção, criou uma conexão privada do tipo alias com destino em um diretório descartável, respondeu à pergunta real sobre o destino, enviou e verificou o arquivo de teste e o removeu. Um arquivo preexistente no destino conservou nome e conteúdo. Outro teste de integração cobre envio, listagem, download, comparação de bytes e remoção condicionada ao digest. O binário de produção continua exigindo o registro e o hash do executável gerenciado.
4. **Regressões A e B.** Os roteiros exercitaram criação de arquivo cifrado, cancelamento e redimensionamento com modal aberto, identidade do arquivo ao chegar outra cópia, exportação da chave com acesso privado, colisão sem sobrescrita, exportação com servidor parado e preparação do comando SCP. O teste por SSH entre dois computadores permanece pendente.

## Reprodução

Para os PTYs são necessários Linux, pyte, Pillow e as fontes DejaVu. As instituições e os dados usados pelos roteiros são temporários. O roteiro de agendamentos aceita um executável B por `--binary` e o sinalizador `--baseline` para obter as imagens anteriores. A opção `--catalogue-program` só é necessária para exercitar um cache disponível quando a descoberta pelo serviço falha.

```sh
cargo build --locked -p elzevir --bin elzevir
python3 tools/panel/capture_schedules.py --output /tmp/elzevir-agendas --catalogue-program /usr/bin/rclone
python3 tools/panel/capture_dashboard.py --output /tmp/elzevir-dashboard
python3 tools/panel/capture_backups.py --output /tmp/elzevir-backups
cargo test --locked -p elzevir --bin elzevir installed_provider_catalogue -- --ignored
cargo fmt --all --check
cargo nextest run --workspace --locked --run-ignored all --no-fail-fast --test-threads 4
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask ci
cargo deny check
```

## Limites da entrega

A homologação de nuvem externa depende de rclone gerenciado funcional e de uma conta de ensaio autorizada. O cenário local não mede OAuth, renovação de token, latência do provedor, callback por SSH nem o navegador no computador da biblioteca. Windows nativo, WSL2, desktop gráfico e tarefas com funcionários permanecem na matriz de aceite.

Ainda cabem em C a edição guiada de planos por conexão e destino, suspensão e retomada com revisão, associação da última tentativa ao plano e orientação de OAuth por ambiente. A previsão da próxima execução precisa considerar a data já reivindicada pelo agendador. As propostas de outras frequências, transferência por navegador e ensaio completo de restauração estão no estudo para decisão própria. O Marco D começa após a revisão de C.
