# Operação do Elzevir

O servidor, o painel e os comandos administrativos pertencem ao mesmo
executável. Cada instituição tem configuração, dados, trava e canal local
próprios. O catálogo continua funcionando quando o painel é fechado.

## Instalação gerenciada

Linux x86_64/ARM64 exige systemd ou OpenRC para registrar o serviço. Windows
x64/ARM64 exige Windows 10 1809+ ou Server 2019+. O banco deve ficar em disco
local. Para instalar no PATH do usuário e rodar pelo terminal, siga a
[instalação rápida](../README.md#instalação-rápida).

Com os scripts e uma release disponíveis:

```sh
sudo bash install.sh --name "Minha biblioteca" --dry-run
sudo bash install.sh --name "Minha biblioteca"
```

No PowerShell elevado:

```powershell
.\install.ps1 -Name 'Minha biblioteca' -Plan
.\install.ps1 -Name 'Minha biblioteca'
```

A senha do administrador é pedida pelo próprio Elzevir. Não a coloque em
argumentos. Os instaladores verificam a release antes de executar o binário,
criam conta limitada para o serviço e preservam instalações existentes.
A primeira criação verifica se a porta está disponível. Firewall é opção
explícita. O instalador não cria exclusão do Defender; o atalho do painel no
menu Iniciar precisa ser aberto com "Executar como administrador" para
controlar o serviço. Reexecutar o instalador usa o fluxo de
atualização com backup; não recria o banco ou o administrador.

Na instituição padrão, Linux usa /etc/elzevir/library.toml e
/var/lib/elzevir/library; Windows usa ProgramData\Elzevir\library.
Instituições adicionais usam --id/-Id, diretórios e executáveis separados.
Escolha portas diferentes para serviços que compartilham a máquina. O plano
mostra os caminhos finais sem baixar nem instalar.

## Painel e estado

```sh
elzevir panel --demo --lang pt-BR
elzevir panel --config CAMINHO/server.toml --lang pt-BR
elzevir status --config CAMINHO/server.toml --json
```

O idioma padrão é inglês; --lang pt-BR seleciona português. --ascii permite
bordas simples em terminais sem os caracteres usados pelo painel.

O painel separa configurações de observações e de testes públicos. A ajuda
de cada seção mostra suas teclas. Operações de escrita têm confirmação;
Esc cancela o formulário antes do envio. Restaurar, atualizar e controlar o
serviço exigem o operador elevado. Tarefas aceitas continuam fora da UI.
O histórico mostra falhas/interrupções, sem conteúdo de senhas ou tokens.

## Backups e recuperação

```sh
elzevir backup now --config CAMINHO/server.toml
elzevir operations --config CAMINHO/server.toml list
elzevir backup key --data-dir CAMINHO/data --output DESTINO-NOVO/backup.key
```

A cópia contém uma imagem SQLite consistente, configurações e segredos
cifrados com age. A chave de recuperação fica fora do arquivo. Exporte-a
explicitamente para um destino privado separado: o arquivo cifrado sozinho
não permite restauração. Índice, logs e certificado mestre Cloudflare não
entram no backup. O índice é reconstruído ao restaurar.

A seção Backup permite criar, verificar, exportar, restaurar e definir
retenção/agenda local e de nuvem. No painel, a restauração gerenciada para o
serviço, preserva os arquivos anteriores e verifica saúde após reiniciar.
Falha de saúde provoca tentativa de retorno à instalação anterior. O comando
backup restore é offline e exige servidor parado e --confirm-restore.

Se uma troca de arquivos for interrompida, o servidor recusa abrir um diário
pendente. Com a instituição parada, a recuperação explícita é:

```sh
elzevir backup recover-restore --data-dir CAMINHO/data --config CAMINHO/server.toml --confirm-restore
```

## Nuvem e endereços públicos

Os instaladores não instalam ferramentas opcionais nem autorizam contas.
No painel, instale rclone para conexões/planos e cloudflared para túneis.
A configuração rclone roda na conta limitada do servidor; mantenha o serviço
em execução para responder aos formulários pelo canal administrativo local.
Transferências usam nomes próprios e retenção de artefatos reconhecidos, sem
sync/purge sobre o remoto. Falha de envio preserva a cópia local.

Domínio é opcional e pode ser escolhido posteriormente, sem recompilar.
Domínio próprio requer autorização da conta Cloudflare e dois hostnames
distintos para catálogo/equipe. Para endereço gratuito, abra **Endereços e acesso**
e pressione **g** no fluxo **Domínio & Túnel (*.elzevir.app)**. O padrão é
https://broker.elzevir.app; deixe o campo de URL vazio para usar esse padrão ou
ELZEVIR_BROKER_URL. Informe biblioteca, contato e slug opcional. Confirme o
cadastro e, se houver revisão pendente, use o link de autorização apresentado.
O painel retoma o provisionamento e mostra os endereços atribuídos e o estado
local do cloudflared. Use **c** para atualizar o estado remoto, **w/W** para
configurar/desativar acesso da equipe, **n** para rotacionar o token e **z**
para desvincular. Testes HTTPS continuam disponíveis em **t**.

A CLI oferece o mesmo fluxo (terminal de administrador/operador):

```sh
elzevir address --config /caminho/server.toml --confirm --wait enroll --institution 'Fictional library' --email contato@example.org --name minha-biblioteca
elzevir address --config /caminho/server.toml status
elzevir address --config /caminho/server.toml --wait refresh
elzevir address --config /caminho/server.toml --confirm --wait staff --username equipe --cidr 192.0.2.0/24
elzevir address --config /caminho/server.toml --confirm --wait renew
```

A inscrição aceita `--broker` ou `--broker-url` para override da URL HTTPS.
A senha remota é lida de forma mascarada, sem argumento de linha de comando.
A identidade Ed25519 fica no arquivo privado `DATA_DIR/secrets/broker.json`.
Instalar/iniciar o programa não registra domínio nem DNS automaticamente;
esses efeitos só ocorrem após o cadastro confirmado pelo operador. O contrato
é validado com respostas simuladas; a validação pública real exige executar
o fluxo contra o serviço e testar DNS/HTTPS.

## Atualizações

A seção Atualizações consulta releases e atualiza Elzevir e ferramentas. O
Elzevir fixa a versão, verifica o pacote, faz backup, para o serviço e verifica
a nova saúde. O executável e o banco anteriores são preservados para rollback.
Atualização de rclone recusa ferramenta em uso. Atualizar cloudflared mantém a
intenção anterior do conector selecionado, sem parar outras instituições.

Após interrupção de atualização, use r na seção Atualizações ou outro binário
Elzevir disponível para revisar/recuperar o executável instalado:

```sh
elzevir recover-update --executable CAMINHO/elzevir --dry-run
elzevir recover-update --executable CAMINHO/elzevir --confirm-recovery --wait
```

O diário privado identifica a instituição. A recuperação recusa outro config
ou arquivos divergentes. Uma versão nova já saudável apenas conclui o diário,
preservando suas escritas. Uma recuperação incompleta mantém os arquivos de
segurança para nova tentativa.

## Situação do roadmap operacional

| Etapas | Código entregue | Validação externa posterior |
|---|---|---|
| A1–A4 | Configuração, init, saúde/logs/status e IPC local | Uso na instalação real |
| A5–A6 | Backup cifrado, verificação, restauração, retenção e agendas | Recuperação com cópias reais do operador |
| A7 | systemd, OpenRC e SCM Windows, parada com checkpoint | Registro/início em cada sistema nativo |
| A8–A9 | Workflow de quatro alvos, checksums e instaladores | Executar/publicar uma release validada |
| P1–P4 | Painel, observações, diagnóstico e modos de acesso | Avaliação de uso |
| P5 | Túnel de domínio próprio e teste público | Conta Cloudflare, DNS e domínio escolhido |
| P6–P7 | Device Flow do kei-broker, domínio gratuito elzevir.app, equipe remota, recuperação e QR | Validar inscrição, DNS/HTTPS e túnel no serviço público |
| P8–P10 | Central de backup, rclone, crypt e planos de nuvem | Autorizar/testar provedores escolhidos |
| P11 | Atualizações e recuperação do Elzevir e das ferramentas | Pacotes oficiais e execução nativa |
| P12 | Contagem de sessões, reinício de serviço e máquina | Controle nativo na máquina do operador |

Testes locais usam instituições fictícias e adaptadores simulados para
serviços, downloads e nuvem. Compilação de adaptadores para Windows verifica
tipos/APIs, sem executar SCM, ACLs ou processos Windows. O CI possui testes
Windows e verificações do PowerShell 5.1; publicar o PR permite validá-los na
plataforma. macOS/launchd não faz parte dos quatro alvos desta distribuição.
