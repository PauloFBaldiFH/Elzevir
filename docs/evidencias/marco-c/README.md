# Marco C — agendas e conexões de nuvem

Continuidade da proposta consolidada. O PR #275 integrou o Marco B na main em `0b509cd9`. A branch de trabalho é `codex/painel-marco-c`. O commit do estudo é `9cfb0625`, a central de agendas e a navegação da nuvem estão em `0bf0c92d`, a correção do catálogo em `47285011`, o ajuste da leitura em terminal compacto em `71b0e0b9` e a edição protegida da agenda de um plano em `d4120873`. [Estudo e matriz de prioridades](../../evolucao-painel-operacional.md) · [Contrato do painel](../../specs/operacao/painel-operacoes.md) · [Contrato da nuvem](../../specs/operacao/nuvem.md)

## O que o operador consegue fazer

- Abrir **Agendamentos** para consultar a agenda local e os planos externos. Na política local, conferir destino, quantidade, horário e fuso. Em tela larga, a atividade recente agrupa os recibos de backup do servidor. A edição parte da política observada. Se a observação faltar ou mudar antes de confirmar, o painel solicita nova leitura.
- Selecionar um plano e revisar o destino antes de executar, editar retenção ou pausar a agenda. Para reativá-la, escolher um novo horário e fuso. A confirmação mostra a conexão e a pasta, que permanecem vinculadas ao plano. A seleção sobrevive à reordenação da lista. Mudar a conexão ou a pasta invalida a seleção. As consultas e os cancelamentos deixam a fila intacta.
- Navegar por Conexões, Planos, Na nuvem e Histórico usando clique ou Tab/Enter. Consultar provedores, filtrar o catálogo por nome ou descrição, abrir a configuração com o tipo preenchido, responder à pergunta do provedor e revisar operações. O assistente preserva as validações de sessão, opções privadas do rclone e o canal administrativo existente.
- Criar um plano em **Nuvem → Planos → Novo plano**. A lista mostra nome e provedor das conexões prontas. O serviço atribui o ID ao confirmar. O operador escolhe a subpasta, a retenção, o horário e o fuso. A revisão exige que a conexão escolhida continue disponível.
- Consultar a última tentativa do plano em **Agendamentos** ou na aba **Planos**. A TUI mostra o estado do trabalho e o horário do pedido em UTC. Um recibo com destino antigo ou sem destino registrado traz essa indicação. O histórico é limitado aos 64 trabalhos retidos pelo serviço.
- Ler a falha de descoberta no histórico. Ações que dependem de serviço disponível têm proteção também no teclado. As descrições longas do painel de nuvem quebram em linhas e o paginador usa a quantidade de linhas efetivamente desenhadas.

## Antes e depois no executável

| Percurso | Evidência e condições |
| --- | --- |
| Marco B, antes da mudança | [Agendamentos abria um formulário](antes-agendamentos.png) · [Área anterior de nuvem](antes-nuvem.png) |
| Consulta de agendas | [160×45](agendamentos-160x45.png) · [80×24](agendamentos-80x24.png). Política gravada pelo painel e relida pela CLI do servidor. |
| Plano externo | [Plano selecionado em 160×45](plano-selecionado.png) · [80×24](plano-selecionado-80x24.png) · [Edição em tela larga](plano-edicao.png) · [Edição compacta](plano-edicao-80x24.png). A revisão mostra nome, retenção, horário e fuso. |
| Criação guiada | [Escolha da conexão](plano-escolher-conexao-80x24.png) · [Revisão do novo plano](plano-novo-80x24.png). A CLI releu a identidade da conexão e a política gravadas pelo serviço. |
| Resultado da tentativa | [160×45](plano-ultima-tentativa.png) · [80×24](plano-ultima-tentativa-80x24.png). Falha de envio no ensaio com a cópia cifrada local autenticada pela CLI. |
| Central de nuvem | [160×45](nuvem-160x45.png) · [80×24](nuvem-80x24.png) · [Falha registrada ao consultar os provedores](nuvem-falha-consulta.png). A instalação de ensaio começa sem conexões. |
| Nova conexão | [Busca no catálogo real em cache](provedor-filtro.png) · [Busca compacta](provedor-filtro-80x24.png) · [Configuração com o provedor escolhido](provedor-revisao.png). As imagens mostram seleção e revisão. Nenhuma conta Google foi autenticada. |
| Comparação com a intenção visual | [Propostas 05–07 ao lado das telas executáveis](comparacao.png). Em 80 colunas a agenda local fica acima da lista de planos. O catálogo tem filtro e foco por linha. |

## Operações exercitadas

1. **Binário e serviço em PTY Linux.** Uma instituição exclusiva foi inicializada, o serviço escutou em loopback, e o teste enviou cliques SGR de pressionar e soltar. Abrir a página e cancelar o formulário preservou a política original. A edição definiu 23h59 em America/Sao_Paulo e outra conexão à CLI leu os dois valores. Foram verificados 80×24, 100×30, 120×30, 160×45, 180×50 e 200×60. Depois da parada, o clique e o atalho recusaram editar uma política desconhecida.
2. **Falha do serviço e leitura de cache.** A consulta real do catálogo gerou um trabalho do servidor. O instalador oficial do rclone gerenciado também foi exercitado em instituição descartável. O recibo foi de falha e a ferramenta continuou indisponível em seu diretório protegido. Para exercitar separadamente o caso de uma instalação com catálogo prévio, o ensaio carregou apenas a saída pública de `rclone --config /dev/null config providers` no cache da instituição temporária. A pesquisa, o clique na linha do Drive e o cancelamento rodaram na TUI. A fila foi conferida antes e depois.
3. **Rclone real em teste de integração.** No Linux, o teste opcional usa o executável de distribuição em `/usr/bin/rclone` e os mesmos métodos de descoberta, configuração, resposta e teste do worker. O catálogo de 46 provedores dessa versão inclui nomes como `google cloud storage`. O teste inicialmente falhou porque o Elzevir recusava a lista inteira. Com a correção, criou uma conexão privada do tipo alias com destino em um diretório descartável, respondeu à pergunta real sobre o destino, enviou e verificou o arquivo de teste e o removeu. Um arquivo preexistente no destino conservou nome e conteúdo. Outro teste de integração cobre envio, listagem, download, comparação de bytes e remoção condicionada ao digest. O binário de produção continua exigindo o registro e o hash do executável gerenciado.
4. **Regressões A e B.** Os roteiros exercitaram criação de arquivo cifrado, cancelamento e redimensionamento com modal aberto, identidade do arquivo ao chegar outra cópia, exportação da chave com acesso privado, colisão sem sobrescrita, exportação com servidor parado e preparação do comando SCP. O teste por SSH entre dois computadores permanece pendente.

5. **Agenda de um destino existente.** O roteiro criou um alias com o rclone real, confirmou que ele listava um arquivo preexistente e preparou o cadastro desse destino na instituição temporária. Na primeira rodada, a criação do plano passou pela CLI e pelo worker. Na rodada atual, o roteiro abre o seletor da TUI, percorre as conexões com Tab, seleciona com Enter e cancela com Esc. Depois cria o plano com o mouse e confere os valores persistidos por outro cliente. Na TUI, cancelar a pausa preservou o horário anterior. Confirmar retirou a agenda diária. Reativar gravou o novo horário e fuso no mesmo plano, conexão e pasta. Enquanto um novo formulário estava aberto, outro cliente alterou a retenção para 12. A TUI recusou salvar a revisão antiga e a CLI confirmou a retenção de 12. O arquivo preexistente manteve seu conteúdo.

6. **Recibo por plano.** Cancelar a execução conservou a fila. A confirmação seguinte gerou um trabalho com a identidade do plano, a conexão e a pasta selecionados. O estado terminal foi de falha neste ambiente com rclone gerenciado indisponível. A TUI exibiu o resultado e o horário do pedido em 160×45, 80×24 e 80×18. No menor tamanho, o ensaio abriu a revisão da execução e saiu com Esc. A CLI autenticou a cópia local de mesmo ID com a chave da instituição temporária. Testes de regressão cobrem históricos sem consulta, sem tentativa do plano, com destino anterior e com registros antigos que só identificam o plano, inclusive quando o relógio recua.
7. **Orientação de autorização.** Um teste de apresentação exercita as instruções em português e inglês com um endereço fictício. O link temporário do rclone aparece em Conexões enquanto a autorização está ativa. A conexão por terminal tem orientação para encaminhamento local pelo SSH. Os links de autorização do conector Cloudflare aparecem em Endereços. O teste não usa contas externas.

## Reprodução

Para os PTYs são necessários Linux, pyte, Pillow e as fontes DejaVu. As instituições e os dados usados pelos roteiros são temporários. O roteiro de agendamentos aceita um executável B por `--binary` e o sinalizador `--baseline` para obter as imagens anteriores. A opção `--catalogue-program` fornece um rclone real para preparar o alias de disco. Ela também permite exercitar um catálogo já disponível em cache quando a descoberta pelo serviço falha.

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

## Verificação do workspace

O workspace na base do PR #275 passou pela suíte completa de 1.296 testes, incluindo os ensaios opcionais com rclone. A verificação de camadas, a geração dos artefatos, o limite de tamanho de arquivos, os desvios e as preferências do Koha integram `cargo xtask ci`. Formatação, Clippy e cargo-deny completam a validação do repositório.

## Limites da entrega

A homologação de nuvem externa depende de rclone gerenciado funcional e de uma conta de ensaio autorizada. O cenário local não mede OAuth, renovação de token, latência do provedor, callback por SSH nem o navegador no computador da biblioteca. Windows nativo, WSL2, desktop gráfico e tarefas com funcionários permanecem na matriz de aceite.

Para exibir a próxima execução prevista, o painel precisa conhecer a próxima data elegível calculada pelo serviço. Hoje a fila conserva a data civil já reivindicada por plano, mas o canal de consulta expõe somente a política e os trabalhos recentes. A proposta é projetar o próximo horário elegível a partir da regra do agendador e testá-lo com mudança de fuso, horário de verão e reinício. Essa extensão do contrato fica para revisão. As propostas de outras frequências, transferência por navegador e ensaio completo de restauração estão no estudo para decisão própria. O Marco D começa após a revisão de C.
