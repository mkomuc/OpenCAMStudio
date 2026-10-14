
**Machining**

Defaults View beim Start. 

![[Pasted image 20261014143248.png]]

Gesamter Bildschirm zeigt das importierte 3d Modell bzw. die Zeichnung. Als Referenz soll in der XY Ebene ein Gitter mit Bemassung dargestellt werden. 

Am linken Rand wird das Projekt als Tree im overlay angezeigt. Die einzelnen Punkte sollen außer den Hufe/show und Status anzeigen keine weiteren Bedienelemente haben. Die Funktionen können über ein Kontext Menü (Rechte Maus) ausgewählt werden. Einzelne Elemente werden über Drag/drop in der Reihenfolge sortiert (z.b. bei den Operationen). 

**Setup**

![[Bildschirmfoto 2026-10-13 um 18.39.03.png]]

**Art des Setups**

Vorerst wird nur 3 Achsen Operationen unterstützt. Später kann dies durch eine rotationsachse 4 oder auch 5 Achse oder ein Drehbank setup ergänzt werden. 


**Rohteil** 

Die Größe des Rohteils konfigurieren mit mehreren Modes

- Feste Größe: alle Dimensionen werden von User eingegeben
- Relative Größe: zusätzliche Größe in jeder Dimension zur Box des Modells wird angeben
- Von vorherigen setup: das bearbeitete Modell vom vorherigen setup wird als rohteil verwendet
- 
![[Bildschirmfoto 2026-10-13 um 18.39.22.png|366]]![[Bildschirmfoto 2026-10-13 um 18.39.28.png|318]]


**Modell Auswahl**

Wenn ein 3D Modell mit mehreren Körpern geladen wird, soll möglich sein, ein oder mehrere Körper als Modell auszuwählen. Wenn nur ein Körper oder eine Zeichnung vorhanden ist, ist die Auswahl damit klar. 

Die anderen Körper können später z.B. als Fixture ausgewählt werden. 

  

**Modell Orientierung im Rohteil**

Für jede Achse soll die Auswahl da sein, ob es oben, unten bzw. links/rechts oder mittig im Rohteil platziert wird. Zusätzlich kann pro Achse ein Offset angegeben werden. 

  

**Werkstück Koordinatensystem** 

Dropdown Menü mit folgenden Auswahlpunkten

- Modell Koordinatensystem 
- Punkt auf Modell: . Im Modell mit  der Maus ausgewählt werden. hier bei der entsprechende Punkte gehighlighted werden und die Maus darauf eintrasten. Neben den Ecken im Modell soll auch die begrenzungsbox des Werkstücks (in anderer Farbe) angezeigt werden und zur Auswahl stehen
- Punkt auf Rohteil: wie oben nur auf dem rohteil

  

Wenn nicht das  Modell Koordinatensystem ausgewählt ist, muss für eine Achse eine Kante des Modells oder Rohteils ausgewählt werden um die Orientierung zu bestimmen. Der weitere Freiheitsgrad kann durch die Auswahl einer zweiten Kante für die zweite Achse festgelegt werden. Das Koordinatensystem soll mit Orientierung mit Modell und rohteil dargestellt werden. Beim Klick auf einen Pfeil des Koordinatensystems soll sich diese Achse um 180 drehen. Es muss sichergestellt werden, dass das System vollständig constrained ist. 

  

**Material Auswahl**

dem Stock kann ein Material aus der Bibliothek zugewiesen werden. Anhand des Materials wird der entsprechende preset der Speeds&Feeds für das jeweilige Tool vorausgewählt. Dies kann bei der Operation/Toolauswahl natürlich manuell geändert werden. 

  

**Postprocessor**

Hier wird die Maschine und damit der entsprechende Postprocessor für alle Operationen des Setups ausgewählt. 

  

**Fixtures**

Auswahl der Körper, die als Fixture dienen. Diese werden in der Collision detection verwendet.  

  

**Tools**

Anstatt des Modells wird die Tool Lib im Hauptfenster dargestellt (wie es schonmal war (im # Commit 5c510b5)).
  
![[Bildschirmfoto 2026-10-14 um 14.05.06.png]]
  
erweitere jedoch die funktionalität: siehe [[tool-library-implementation-reference]]. die tabs sollen im inspector sein.
nutze die vorhandene visualierung und auch die begrenze auch den tool typ auf die bereits vorhandenen typen. weitere typen können zu einem späteren zeitpunkt hinzugefügt werden.

Import von Fusion 360 Tool Libs von Herstellern soll möglich sein. 
  

**Operationen**

Um eine Operation abzulegen wird wie folgt vorgegangen. 

- Feature (Edge, Face,…) durch klicken im Modell auswählen. Mehrere (gleiche) Features können durch gleichzeitiges Drücken der STRG Taste ausgewählt werden. 
- Im Ribbon die gewünschte Operation auswählen und konfigurieren
- Alternativ: erst die Operation im ribbon auswählen, dann auf Features klicken und die Auswahl treffen.

Menü für eine Operation ist in folgende Tabs unterteilt: je nach Operation können die Parameter in den jeweiligen Gruppen unterschiedlich sein. Nicht alle Parameter werden zur Zeit vom Programm unterstützt. 
Analysiere welche Parameter pro Operation im Backend zur Verfügung stehen und stelle die entsprechenden Parameter zur Verfügung.
Implementierung weiterer Parameter ist für die Zukunft.

Hier als Beispiel die 2D Kontur:

**Tab Werkzeug**

![[Pasted image 20261014142246.png]]

**Tab Multi-Achsen**
für spätere implementierung

**Tab Geometrie**
Auswahl (mehrerer) Geometrien aus dem Modell.
![[Pasted image 20261014142358.png]]


**Tab Höhen**

![[Pasted image 20261014142545.png]]


**Tab Durchgänge**

![[Pasted image 20261014142633.png]]



**Tab Anfahr-Wegfahrbewegungen**

![[Pasted image 20261014142719.png]]