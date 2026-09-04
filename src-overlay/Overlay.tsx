// §9 durum göstergesi — Faz 1a spike: statik "Kayıt" (nefes alan halka).
// Varsayılan görünmez/boş tasarım: pencere transparan + visible:false, bu
// yüzden durum yalnızca Rust tarafı gösterdiğinde belirir. Tıklama geçirgen
// kılamadığımız için (KWin input passthrough yok) overlay etkileşimli değil.
export default function Overlay() {
  return (
    <div className="flex h-full w-full items-center justify-center">
      <div className="breathing-ring" role="status" aria-label="recording" />
    </div>
  );
}