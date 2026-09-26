# Nexo Web

Nova base web do Nexo com interface própria inspirada em apps modernos de comunicação e uma camada realtime baseada no VDO.Ninja via IFRAME API.

## Funcionalidades

- criar sala com código aleatório
- entrar por código ou link de convite
- chamada WebRTC embutida usando VDO.Ninja
- microfone, câmera, compartilhamento de tela e encerramento
- chat via API do VDO.Ninja
- troca de bitrate em tempo real
- estatísticas de conexão via getStats
- persistência local do nome de exibição
- layout responsivo para desktop e mobile

## Arquitetura

O Nexo controla a experiência visual e o fluxo de produto. O VDO.Ninja fica encapsulado no iframe como engine realtime. Isso evita duplicar a parte mais complexa de WebRTC neste primeiro corte e facilita substituir depois os endpoints públicos por infraestrutura própria.

## Dependência externa

Este diretório não incorpora nem redistribui o core AGPL do VDO.Ninja; ele usa a IFRAME API e o serviço hospedado em https://vdo.ninja.

O uso do serviço hospedado do VDO.Ninja é separado da licença do código-fonte e deve respeitar os termos do serviço. Para independência total, a etapa seguinte é apontar o Nexo para uma implantação própria de frontend/signaling/TURN/SFU compatível.
